/* Exercise the native multi-account queue without a socket or physical card. */
static unsigned batch_calls,batch_commands;
static int batch_failure;
static int batch_lookup(const AB_BANKING *ab,uint32_t id,AB_ACCOUNT_SPEC **out) {
  (void)ab;
  if(id<42 || id>44) return GWEN_ERROR_NOT_FOUND;
  AB_ACCOUNT_SPEC *a=AB_AccountSpec_new(); AB_AccountSpec_SetUniqueId(a,id); AB_AccountSpec_SetBackendName(a,"aqhbci");
  for(unsigned i=0;i<(id==44?1u:2u);i++) {
    AB_TRANSACTION_LIMITS *limit=AB_TransactionLimits_new();
    AB_TransactionLimits_SetCommand(limit,i?AB_Transaction_CommandGetTransactions:AB_Transaction_CommandGetBalance);
    AB_AccountSpec_AddTransactionLimits(a,limit);
  }
  *out=a;return 0;
}
static int batch_send(AB_PROVIDER *provider,AB_PROVIDERQUEUE *queue,AB_IMEXPORTER_CONTEXT *ctx) {
  (void)provider; batch_calls++;
  for(AB_ACCOUNTQUEUE *a=AB_ProviderQueue_GetFirstAccountQueue(queue);a;a=AB_AccountQueue_List_Next(a)) {
    unsigned id=AB_AccountQueue_GetAccountId(a);
    AB_IMEXPORTER_ACCOUNTINFO *info=AB_ImExporterAccountInfo_new(); AB_ImExporterAccountInfo_SetAccountId(info,id);
    AB_ImExporterContext_AddAccountInfo(ctx,info);
    AB_TRANSACTION_LIST2_ITERATOR *it=AB_Transaction_List2_First(AB_AccountQueue_GetTransactionList(a));
    while(it) {
      AB_TRANSACTION *t=AB_Transaction_List2Iterator_Data(it); batch_commands++;
      int command=AB_Transaction_GetCommand(t);
      assert(AB_Transaction_GetUniqueAccountId(t)==id);
      assert(AB_Transaction_GetUniqueId(t)>0 && AB_Transaction_GetStatus(t)==AB_Transaction_StatusEnqueued);
      assert(command==AB_Transaction_CommandGetBalance || command==AB_Transaction_CommandGetTransactions);
      if(command==AB_Transaction_CommandGetTransactions) {
        assert(GWEN_Date_GetYear(AB_Transaction_GetFirstDate(t))==2026);
        assert(GWEN_Date_GetDay(AB_Transaction_GetFirstDate(t))==(id==42?1:2));
      }
      AB_Transaction_SetStatus(t,batch_failure==1 && id==43?AB_Transaction_StatusError:AB_Transaction_StatusAccepted);
      if(!AB_Transaction_List2Iterator_Next(it)) break;
    }
    AB_Transaction_List2Iterator_free(it);
    AB_BALANCE *balance=AB_Balance_new(); AB_Balance_SetType(balance,AB_Balance_TypeBooked);
    AB_VALUE *value=AB_Value_fromDouble((double)id); AB_Value_SetCurrency(value,"EUR"); AB_Balance_SetValue(balance,value); AB_Value_free(value);
    GWEN_DATE *date=GWEN_Date_fromString("20261117"); AB_Balance_SetDate(balance,date); GWEN_Date_free(date);
    AB_ImExporterAccountInfo_AddBalance(info,balance);
  }
  return batch_failure==2?GWEN_ERROR_GENERIC:0;
}
static void batch_set(const char *key,const char *value) {
  BC_FIELD *field=(BC_FIELD *)bc_field(&request,key);
  if(!field) {bc_string(&request,key,value);return;}
  bc_wipe(field->value,field->size);free(field->value);
  field->size=(uint32_t)strlen(value);field->value=(unsigned char *)strdup(value);
}
static void batch_request(void) {
  bc_free(&request); request=event("read_batch"); batch_set("op","read_batch"); batch_set("count","2");
  for(unsigned i=0;i<2;i++) {
    char key[32]; snprintf(key,sizeof(key),"account%u",i); bc_number(&request,key,42+i);
    snprintf(key,sizeof(key),"balance%u",i); bc_number(&request,key,1);
    snprintf(key,sizeof(key),"transactions%u",i); bc_number(&request,key,1);
    snprintf(key,sizeof(key),"from%u",i); batch_set(key,i?"20261102":"20261101");
  }
}
static void test_card_read_batch(AB_BANKING *bank) {
  BC_FRAME saved=request; memset(&request,0,sizeof(request)); int old=card_session;card_session=1;
  batch_request(); // 'event' belongs to this test fixture, not subsequent IPC
  assert(run_read_batch_with(bank,batch_lookup,batch_send)==0);
  assert(batch_calls==1 && batch_commands==4); // four read jobs, one outbox dispatch
  batch_failure=1;
  assert(run_read_batch_with(bank,batch_lookup,batch_send)==0); // per-account rejection, never retransmitted
  assert(batch_calls==2 && batch_commands==8);
  batch_failure=2;
  assert(run_read_batch_with(bank,batch_lookup,batch_send)==GWEN_ERROR_GENERIC);
  assert(batch_calls==3); batch_failure=0;
  batch_set("account1","44"); batch_set("balance1","0");
  assert(run_read_batch_with(bank,batch_lookup,batch_send)==0); // unsupported transaction stays local
  assert(batch_calls==4 && batch_commands==14);
  const char *keys[]={"account1","account1","account1","count","count","balance0","from0"};
  const char *values[]={"42","4294967296","999","0","17","2","invalid"};
  for(unsigned i=0;i<sizeof(keys)/sizeof(keys[0]);i++) {
    batch_request();batch_set(keys[i],values[i]);
    assert(run_read_batch_with(bank,batch_lookup,batch_send)<0 && batch_calls==4);
  }
  batch_request();card_session=0;assert(run_read_batch_with(bank,batch_lookup,batch_send)==GWEN_ERROR_INVALID && batch_calls==4);
  BC_FRAME next={0};bc_string(&next,"op","read_batch");bc_string(&next,"count","2");bc_string(&next,"account0","42");
  assert(session_request_allowed(&next));bc_string(&next,"native","forbidden");assert(!session_request_allowed(&next));bc_free(&next);
  // A forged queue containing a payment or a cross-account command is rejected
  // before dispatch even if its outer operation was a read.
  AB_PROVIDERQUEUE *queue=AB_ProviderQueue_new();AB_ProviderQueue_SetProviderName(queue,"aqhbci");
  AB_ACCOUNTQUEUE *a=AB_AccountQueue_new();AB_AccountQueue_SetAccountId(a,42);
  AB_ACCOUNT_SPEC *spec=NULL;assert(batch_lookup(bank,42,&spec)==0);AB_AccountQueue_SetAccountSpec(a,spec);AB_ProviderQueue_AddAccountQueue(queue,a);
  AB_TRANSACTION *t=AB_Transaction_new();AB_Transaction_SetUniqueAccountId(t,42);AB_Transaction_SetCommand(t,AB_Transaction_CommandSepaTransfer);AB_AccountQueue_AddTransaction(a,t);
  AB_IMEXPORTER_CONTEXT *ctx=AB_ImExporterContext_new();
  assert(send_read_queue_with(bank,queue,ctx,batch_send)==GWEN_ERROR_INVALID);
  AB_Transaction_SetCommand(t,AB_Transaction_CommandGetBalance);AB_Transaction_SetUniqueAccountId(t,43);
  assert(send_read_queue_with(bank,queue,ctx,batch_send)==GWEN_ERROR_INVALID && batch_calls==4);
  AB_ProviderQueue_free(queue);AB_Transaction_free(t);AB_ImExporterContext_free(ctx);
  bc_free(&request);request=saved;card_session=old;
}
