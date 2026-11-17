/* Read-only batches share one AqHBCI outbox. AqBanking owns dialog grouping,
 * bank message limits, signatures and pagination; no wire-level shortcuts. */
#define BC_READ_BATCH_MAX 16

static int read_batch_number(const char *key,unsigned max,unsigned *value) {
  const char *s=bc_text(&request,key); unsigned n=0;
  if(!*s) return GWEN_ERROR_INVALID;
  for(;*s;s++) {
    if(*s<'0' || *s>'9' || (unsigned)(*s-'0')>max || n>(max-(unsigned)(*s-'0'))/10) return GWEN_ERROR_INVALID;
    n=n*10+(unsigned)(*s-'0');
  }
  *value=n; return 0;
}
static int read_batch_key(const char *key) {
  if(!strcmp(key,"op") || !strcmp(key,"count")) return 1;
  const char *fields[]={"account","balance","transactions","from"};
  for(unsigned i=0;i<BC_READ_BATCH_MAX;i++) for(unsigned j=0;j<4;j++) {
    char expected[32]; snprintf(expected,sizeof(expected),"%s%u",fields[j],i);
    if(!strcmp(key,expected)) return 1;
  }
  return 0;
}
static int send_read_queue_with(AB_BANKING *ab,AB_PROVIDERQUEUE *queue,AB_IMEXPORTER_CONTEXT *ctx,
                               int (*send)(AB_PROVIDER *,AB_PROVIDERQUEUE *,AB_IMEXPORTER_CONTEXT *)) {
  // Validate the whole queue before any network/card operation.
  if(strcmp(str(AB_ProviderQueue_GetProviderName(queue)),"aqhbci")) return GWEN_ERROR_INVALID;
  for(AB_ACCOUNTQUEUE *a=AB_ProviderQueue_GetFirstAccountQueue(queue);a;a=AB_AccountQueue_List_Next(a)) {
    const AB_ACCOUNT_SPEC *spec=AB_AccountQueue_GetAccountSpec(a);
    if(!spec || strcmp(str(AB_AccountSpec_GetBackendName(spec)),"aqhbci") ||
       AB_AccountSpec_GetUniqueId(spec)!=AB_AccountQueue_GetAccountId(a)) return GWEN_ERROR_INVALID;
    AB_TRANSACTION_LIST2_ITERATOR *it=AB_Transaction_List2_First(AB_AccountQueue_GetTransactionList(a));
    while(it) {
      AB_TRANSACTION *t=AB_Transaction_List2Iterator_Data(it);
      int cmd=AB_Transaction_GetCommand(t);
      if((cmd!=AB_Transaction_CommandGetBalance && cmd!=AB_Transaction_CommandGetTransactions) ||
         AB_Transaction_GetUniqueAccountId(t)!=AB_AccountQueue_GetAccountId(a)) {
        AB_Transaction_List2Iterator_free(it); return GWEN_ERROR_INVALID;
      }
      if(!AB_Transaction_List2Iterator_Next(it)) break;
    }
    AB_Transaction_List2Iterator_free(it);
  }
  AB_PROVIDER *provider=AB_Banking_BeginUseProvider(ab,"aqhbci");
  if(!provider) return GWEN_ERROR_NOT_FOUND;
  int rv=send(provider,queue,ctx);
  int ended=AB_Banking_EndUseProvider(ab,provider);
  return rv==0?ended:rv;
}
static int run_read_batch_with(AB_BANKING *ab,
    int (*lookup)(const AB_BANKING *,uint32_t,AB_ACCOUNT_SPEC **),
    int (*send)(AB_PROVIDER *,AB_PROVIDERQUEUE *,AB_IMEXPORTER_CONTEXT *)) {
  if(!card_session) return GWEN_ERROR_INVALID;
  unsigned count=0;
  if(read_batch_number("count",BC_READ_BATCH_MAX,&count)<0 || !count) return GWEN_ERROR_INVALID;
  struct { unsigned id; AB_TRANSACTION *jobs[2]; int codes[2]; } items[BC_READ_BATCH_MAX]={0};
  AB_PROVIDERQUEUE *queue=AB_ProviderQueue_new(); AB_ProviderQueue_SetProviderName(queue,"aqhbci");
  AB_IMEXPORTER_CONTEXT *ctx=AB_ImExporterContext_new();
  int rv=0, jobs=0;
  for(unsigned i=0;i<count;i++) {
    char key[32]; unsigned id=0, wanted[2]={0};
    snprintf(key,sizeof(key),"account%u",i);
    if(read_batch_number(key,UINT32_MAX,&id)<0 || !id) {rv=GWEN_ERROR_INVALID;goto cleanup_batch;}
    for(unsigned j=0;j<i;j++) if(items[j].id==id) {rv=GWEN_ERROR_INVALID;goto cleanup_batch;}
    items[i].id=id;
    snprintf(key,sizeof(key),"balance%u",i);
    if(read_batch_number(key,1,&wanted[0])<0) {rv=GWEN_ERROR_INVALID;goto cleanup_batch;}
    snprintf(key,sizeof(key),"transactions%u",i);
    if(read_batch_number(key,1,&wanted[1])<0 || !(wanted[0] || wanted[1])) {rv=GWEN_ERROR_INVALID;goto cleanup_batch;}
    AB_ACCOUNT_SPEC *spec=NULL;
    rv=lookup(ab,id,&spec); if(rv<0) goto cleanup_batch;
    if(!spec || AB_AccountSpec_GetUniqueId(spec)!=id || strcmp(str(AB_AccountSpec_GetBackendName(spec)),"aqhbci")) {
      AB_AccountSpec_free(spec);rv=GWEN_ERROR_INVALID;goto cleanup_batch;
    }
    AB_ACCOUNTQUEUE *account=AB_AccountQueue_new();
    AB_AccountQueue_SetAccountId(account,id); AB_AccountQueue_SetAccountSpec(account,spec);
    AB_ProviderQueue_AddAccountQueue(queue,account);
    int before_jobs=jobs;
    for(unsigned j=0;j<2;j++) if(wanted[j]) {
      int command=j?AB_Transaction_CommandGetTransactions:AB_Transaction_CommandGetBalance;
      if(!AB_AccountSpec_GetTransactionLimitsForCommand(spec,command)) {items[i].codes[j]=GWEN_ERROR_NOT_SUPPORTED;continue;}
      AB_TRANSACTION *t=items[i].jobs[j]=AB_Transaction_new();
      AB_Banking_FillTransactionFromAccountSpec(t,spec);
      AB_Transaction_SetCommand(t,command); AB_Transaction_SetUniqueId(t,AB_Banking_ReserveJobId(ab));
      AB_Transaction_SetRefUniqueId(t,0); AB_Transaction_SetStatus(t,AB_Transaction_StatusEnqueued);
      if(j) {
        snprintf(key,sizeof(key),"from%u",i);
        GWEN_DATE *date=GWEN_Date_fromString(bc_text(&request,key));
        if(!date) {rv=GWEN_ERROR_INVALID;goto cleanup_batch;}
        AB_Transaction_SetFirstDate(t,date); GWEN_Date_free(date);
      }
      AB_AccountQueue_AddTransaction(account,t); jobs++;
    }
    if(jobs==before_jobs) {AB_AccountQueue_List_Del(account);AB_AccountQueue_free(account);}
  }
  if(jobs) {
    rv=send_read_queue_with(ab,queue,ctx,send);
    if(was_cancelled) rv=GWEN_ERROR_USER_ABORTED;
    else if(certificate_rejected) rv=GWEN_ERROR_SSL_SECURITY;
    else if(bad_pin) rv=GWEN_ERROR_GENERIC;
    if(rv<0) goto cleanup_batch;
  }
  for(unsigned i=0;i<count;i++) {
    for(unsigned j=0;j<2;j++) if(items[i].jobs[j] && AB_Transaction_GetStatus(items[i].jobs[j])!=AB_Transaction_StatusAccepted)
      items[i].codes[j]=GWEN_ERROR_GENERIC;
    BC_FRAME result=event("read_result"); bc_number(&result,"account",items[i].id);
    bc_number(&result,"balance_code",items[i].codes[0]); bc_number(&result,"transactions_code",items[i].codes[1]);
    bc_string(&result,"notice",last_notice); bc_number(&result,"bad_pin",bad_pin); bc_send(&result);
    for(AB_IMEXPORTER_ACCOUNTINFO *a=AB_ImExporterContext_GetFirstAccountInfo(ctx);a;a=AB_ImExporterAccountInfo_List_Next(a)) {
      if(AB_ImExporterAccountInfo_GetAccountId(a)!=items[i].id) continue;
      rv=emit_account_results(a); if(rv<0) goto cleanup_batch;
    }
  }
cleanup_batch:
  AB_ProviderQueue_free(queue); AB_ImExporterContext_free(ctx);
  for(unsigned i=0;i<count;i++) for(unsigned j=0;j<2;j++) AB_Transaction_free(items[i].jobs[j]);
  return rv;
}
