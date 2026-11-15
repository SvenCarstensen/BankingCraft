/* SPDX-License-Identifier: GPL-3.0-only
 * Exercise the real AqBanking VoP dialog through our adapter with no sockets.
 */
#define main bankingcraft_worker_main
#include "main.c"
#undef main
#include <aqhbci/dialogs/dlg_vop.h>
#include <aqhbci/msglayer/dialog_l.h>
#include <aqhbci/banking/provider_job.h>
#include <aqhbci/joblayer/job_l.h>
#include <aqhbci/joblayer/jobqueue_account.h>
#include <aqbanking/banking_imex.h>
#include <gwenhywfar/ct_be.h>
#include <gwenhywfar/ctplugin_be.h>
#include <assert.h>

static GWEN_CRYPT_TOKEN_CONTEXT *synthetic_contexts[3];
static int synthetic_context_error, context_resolutions, adduser_calls, expected_record, expected_token;
static int synthetic_context_ids(GWEN_CRYPT_TOKEN *token,uint32_t *ids,uint32_t *count,uint32_t gid) {
  (void)token;(void)gid;
  if(synthetic_context_error) return synthetic_context_error;
  assert(*count>=3); ids[0]=2; ids[1]=4; ids[2]=7; *count=3; return 0;
}
static const GWEN_CRYPT_TOKEN_CONTEXT *synthetic_context(GWEN_CRYPT_TOKEN *token,uint32_t id,uint32_t gid) {
  (void)token;(void)gid;
  return id==2?synthetic_contexts[0]:id==4?synthetic_contexts[1]:id==7?synthetic_contexts[2]:NULL;
}
static int synthetic_token_lifecycle(GWEN_CRYPT_TOKEN *token,int flags,uint32_t gid) {
  (void)token;(void)flags;(void)gid;return 0;
}
static int synthetic_resolve(uint32_t *selected,GWEN_BUFFER *name) {
  context_resolutions++;
  GWEN_CRYPT_TOKEN *token=GWEN_Crypt_Token_new(GWEN_Crypt_Token_Device_Card,"synthetic","synthetic-card");
  GWEN_Crypt_Token_SetGetContextIdListFn(token,synthetic_context_ids);
  GWEN_Crypt_Token_SetGetContextFn(token,synthetic_context);
  GWEN_Crypt_Token_SetOpenFn(token,synthetic_token_lifecycle);
  GWEN_Crypt_Token_SetCloseFn(token,synthetic_token_lifecycle);
  assert(GWEN_Crypt_Token_Open(token,0,0)==0);
  int rv=match_card_context(token,bc_text(&request,"blz"),bc_text(&request,"user"),selected);
  if(rv==0) GWEN_Buffer_AppendString(name,GWEN_Crypt_Token_GetTokenName(token));
  assert(GWEN_Crypt_Token_Close(token,0,0)==0);
  GWEN_Crypt_Token_free(token); return rv;
}
static int synthetic_adduser(AB_BANKING *bank,const char *backend,int argc,char **argv) {
  (void)bank; adduser_calls++;
  assert(!strcmp(backend,"aqhbci") && !strcmp(argv[0],"adduser"));
  int found_record=0,found_token=0;
  for(int i=1;i<argc;i++) {
    if(!strncmp(argv[i],"--context=",10)) {assert(atoi(argv[i]+10)==expected_record);found_record++;}
    if(!strcmp(argv[i],"-n")) {assert(i+1<argc && !strcmp(argv[++i],"synthetic-card"));found_token++;}
  }
  assert(found_record==1 && found_token==expected_token); return 0;
}
static void test_automatic_card_context(AB_BANKING *bank) {
  for(unsigned i=0;i<3;i++) {
    synthetic_contexts[i]=GWEN_Crypt_Token_Context_new();
    GWEN_Crypt_Token_Context_SetServiceId(synthetic_contexts[i],i==0?"87654321":"12345678");
    GWEN_Crypt_Token_Context_SetUserId(synthetic_contexts[i],i==2?"another-user":"synthetic-user");
  }
  request=event("configure");
  bc_string(&request,"blz","12345678");bc_string(&request,"user","synthetic-user");
  bc_string(&request,"medium","zkacard");bc_string(&request,"record","0");
  expected_record=4;expected_token=1;
  assert(ensure_user_with(bank,synthetic_resolve,synthetic_adduser)==0);
  assert(context_resolutions==1 && adduser_calls==1); // ID 4, not 0, 1 or first entry 2

  synthetic_context_error=GWEN_ERROR_USER_ABORTED;
  assert(ensure_user_with(bank,synthetic_resolve,synthetic_adduser)==GWEN_ERROR_USER_ABORTED);
  assert(adduser_calls==1);synthetic_context_error=0;
  GWEN_Crypt_Token_Context_SetUserId(synthetic_contexts[1],"unrelated-user");
  assert(ensure_user_with(bank,synthetic_resolve,synthetic_adduser)==GWEN_ERROR_NOT_FOUND);
  assert(adduser_calls==1);
  GWEN_Crypt_Token_Context_SetUserId(synthetic_contexts[1],"synthetic-user");
  GWEN_Crypt_Token_Context_SetUserId(synthetic_contexts[2],"synthetic-user");
  assert(ensure_user_with(bank,synthetic_resolve,synthetic_adduser)==GWEN_ERROR_INVALID);
  assert(adduser_calls==1); // ambiguous matches never dispatch
  bc_free(&request);

  request=event("configure");bc_string(&request,"medium","zkacard");bc_string(&request,"record","7");
  expected_record=7;expected_token=0;
  assert(ensure_user_with(bank,synthetic_resolve,synthetic_adduser)==0);
  assert(context_resolutions==4 && adduser_calls==2); // explicit IDs remain unchanged
  bc_free(&request);
  request=event("configure");bc_string(&request,"medium","pintan");bc_string(&request,"record","1");
  expected_record=1;
  assert(ensure_user_with(bank,synthetic_resolve,synthetic_adduser)==0);
  assert(context_resolutions==4 && adduser_calls==3); // no card operations for PIN/TAN
  bc_free(&request);
  for(unsigned i=0;i<3;i++) GWEN_Crypt_Token_Context_free(synthetic_contexts[i]);
  begin_stage("initialize");
}

static int card_opens,card_closes,card_dispatches;
static int session_token_open(GWEN_CRYPT_TOKEN *token,int flags,uint32_t gid) {
  (void)token;(void)flags;(void)gid; card_opens++; return 0;
}
static int session_token_close(GWEN_CRYPT_TOKEN *token,int flags,uint32_t gid) {
  (void)token;(void)flags;(void)gid; card_closes++; return 0;
}
static GWEN_CRYPT_TOKEN *session_token_factory(GWEN_PLUGIN *plugin,const char *name) {
  (void)plugin;
  GWEN_CRYPT_TOKEN *token=GWEN_Crypt_Token_new(GWEN_Crypt_Token_Device_Card,"session-test",name);
  GWEN_Crypt_Token_SetOpenFn(token,session_token_open);
  GWEN_Crypt_Token_SetCloseFn(token,session_token_close);
  return token;
}
static int synthetic_read_send(AB_PROVIDER *provider,AB_PROVIDERQUEUE *queue,AB_IMEXPORTER_CONTEXT *ctx) {
  (void)ctx; card_dispatches++;
  assert(!strcmp(AB_ProviderQueue_GetProviderName(queue),"aqhbci"));
  AB_ACCOUNTQUEUE *account=AB_ProviderQueue_GetFirstAccountQueue(queue); assert(account);
  assert(!AB_AccountQueue_List_Next(account));
  assert(AB_AccountQueue_GetAccountId(account)==42);
  assert(AB_AccountSpec_GetUniqueId(AB_AccountQueue_GetAccountSpec(account))==42);
  AB_TRANSACTION_LIST2_ITERATOR *it=AB_Transaction_List2_First(AB_AccountQueue_GetTransactionList(account)); assert(it);
  AB_TRANSACTION *t=AB_Transaction_List2Iterator_Data(it);
  assert(AB_Transaction_GetUniqueAccountId(t)==42);
  assert(AB_Transaction_GetStatus(t)==AB_Transaction_StatusEnqueued);
  assert(!AB_Transaction_List2Iterator_Next(it)); AB_Transaction_List2Iterator_free(it);
  GWEN_CRYPT_TOKEN *token=NULL;
  assert(AB_Banking_GetCryptToken(AB_Provider_GetBanking(provider),"session-test","synthetic-card",&token)==0);
  if(!GWEN_Crypt_Token_IsOpen(token)) assert(GWEN_Crypt_Token_Open(token,0,0)==0);
  AB_Transaction_SetStatus(t,AB_Transaction_StatusAccepted);
  return 0;
}
static void test_card_read_session(AB_BANKING *bank) {
  GWEN_PLUGIN_MANAGER *pm=GWEN_PluginManager_FindPluginManager("ct"); assert(pm);
  GWEN_PLUGIN *plugin=GWEN_Crypt_Token_Plugin_new(pm,GWEN_Crypt_Token_Device_Card,"session-test",NULL);
  GWEN_Crypt_Token_Plugin_SetCreateTokenFn(plugin,session_token_factory);
  GWEN_PluginManager_AddPlugin(pm,plugin);
  AB_ACCOUNT_SPEC *account=AB_AccountSpec_new(); AB_AccountSpec_SetUniqueId(account,42);
  AB_AccountSpec_SetBackendName(account,"aqhbci");
  AB_TRANSACTION *t=AB_Transaction_new(); AB_Transaction_SetUniqueAccountId(t,42);
  AB_IMEXPORTER_CONTEXT *ctx=AB_ImExporterContext_new();
  const int commands[]={AB_Transaction_CommandGetBalance,AB_Transaction_CommandGetTransactions};
  for(unsigned i=0;i<2;i++) {
    AB_Transaction_SetCommand(t,commands[i]);
    assert(send_card_read_with(bank,account,t,ctx,synthetic_read_send)==0);
    assert(AB_Transaction_GetStatus(t)==AB_Transaction_StatusAccepted);
  }
  assert(card_dispatches==2 && card_opens==1 && card_closes==0);
  AB_Transaction_SetCommand(t,AB_Transaction_CommandSepaTransfer);
  assert(send_card_read_with(bank,account,t,ctx,synthetic_read_send)==GWEN_ERROR_INVALID);
  AB_Transaction_SetCommand(t,AB_Transaction_CommandGetBalance);
  AB_Transaction_SetUniqueAccountId(t,43);
  assert(send_card_read_with(bank,account,t,ctx,synthetic_read_send)==GWEN_ERROR_INVALID);
  assert(card_dispatches==2);
  AB_Banking_ClearCryptTokenList(bank); assert(card_closes==1);
  AB_Transaction_SetUniqueAccountId(t,42);
  assert(send_card_read_with(bank,account,t,ctx,synthetic_read_send)==0);
  assert(card_opens==2); // authentication is not reused by a new session
  AB_Banking_ClearCryptTokenList(bank); assert(card_closes==2);
  AB_ImExporterContext_free(ctx); AB_Transaction_free(t); AB_AccountSpec_free(account);
  const char *denied[]={"transfer","scheduled","create_standing","delete_standing","reset_tan","configure","read_card"};
  for(unsigned i=0;i<sizeof(denied)/sizeof(denied[0]);i++) {
    BC_FRAME next={0};bc_string(&next,"op",denied[i]);
    assert(!session_request_allowed(&next));bc_free(&next);
  }
  BC_FRAME next={0};bc_string(&next,"op","balance");bc_string(&next,"account","42");
  assert(session_request_allowed(&next));bc_string(&next,"medium","pintan");
  assert(!session_request_allowed(&next));bc_free(&next);
}

static void test_camt_entry_amounts(AB_BANKING *bank) {
  // Synthetic camt.052.001.08, imported through the same AqBanking profile
  // as HKCAZ. The booked amount belongs to Ntry, not optional TxDtls.
  const char *xml="<?xml version=\"1.0\"?>"
    "<Document xmlns=\"urn:iso:std:iso:20022:tech:xsd:camt.052.001.08\"><BkToCstmrAcctRpt><Rpt>"
    "<Id>SYNTHETIC</Id><Acct><Id><IBAN>DE02120300000000202051</IBAN></Id><Ccy>EUR</Ccy></Acct>"
    "<Bal><Tp><CdOrPrtry><Cd>CLBD</Cd></CdOrPrtry></Tp><Amt Ccy=\"EUR\">20.00</Amt>"
    "<CdtDbtInd>CRDT</CdtDbtInd><Dt><Dt>2026-11-01</Dt></Dt></Bal>"
    "<Bal><Tp><CdOrPrtry><Cd>INFORMATION</Cd></CdOrPrtry></Tp></Bal>"
    "<Ntry><Amt Ccy=\"EUR\">12.34</Amt><CdtDbtInd>DBIT</CdtDbtInd><Sts><Cd>BOOK</Cd></Sts>"
    "<BookgDt><Dt>2026-11-01</Dt></BookgDt><NtryDtls><TxDtls>"
    "<Refs><EndToEndId>SYNTHETIC-DEBIT</EndToEndId></Refs>"
    "<AmtDtls><TxAmt><Amt Ccy=\"USD\">99.99</Amt></TxAmt></AmtDtls>"
    "<RmtInf><Ustrd>Synthetic debit</Ustrd></RmtInf></TxDtls></NtryDtls></Ntry>"
    "<Ntry><Amt Ccy=\"EUR\">56.78</Amt><CdtDbtInd>CRDT</CdtDbtInd><Sts><Cd>BOOK</Cd></Sts>"
    "<NtryDtls><TxDtls><Refs><EndToEndId>SYNTHETIC-CREDIT</EndToEndId></Refs></TxDtls></NtryDtls></Ntry>"
    "<Ntry><Amt Ccy=\"EUR\">3.21</Amt><CdtDbtInd>DBIT</CdtDbtInd><Sts><Cd>PDNG</Cd></Sts></Ntry>"
    "<Ntry><Amt Ccy=\"USD\">0.00</Amt><CdtDbtInd>CRDT</CdtDbtInd><Sts><Cd>BOOK</Cd></Sts></Ntry>"
    "</Rpt></BkToCstmrAcctRpt></Document>";
  AB_IMEXPORTER_CONTEXT *ctx=AB_ImExporterContext_new();
  assert(AB_Banking_ImportFromBufferLoadProfile(bank,"xml",ctx,"default",NULL,(const uint8_t *)xml,(uint32_t)strlen(xml))==0);
  AB_IMEXPORTER_ACCOUNTINFO *account=AB_ImExporterContext_GetFirstAccountInfo(ctx); assert(account);
  const char *amounts[]={"-12.34","56.78","-3.21","0.00"};
  unsigned count=0;
  for(AB_TRANSACTION *t=AB_Transaction_List_First(AB_ImExporterAccountInfo_GetTransactionList(account));t;t=AB_Transaction_List_Next(t)) {
    assert(count<4);
    const AB_VALUE *value=AB_Transaction_GetValue(t); assert(value);
    AB_VALUE *expected=AB_Value_fromString(amounts[count]); assert(expected);
    assert(AB_Value_Compare(value,expected)==0); AB_Value_free(expected);
    assert(!strcmp(AB_Value_GetCurrency(value),count==3?"USD":"EUR"));
    assert(AB_Transaction_GetType(t)==(count==2?AB_Transaction_TypeNotedStatement:AB_Transaction_TypeStatement));
    if(count==0) assert(!strcmp(AB_Transaction_GetPurpose(t),"Synthetic debit"));
    count++;
  }
  assert(count==4);
  AB_BALANCE *extra=AB_Balance_List_Next(AB_ImExporterAccountInfo_GetFirstBalance(account));
  assert(extra && AB_Balance_GetType(extra)==AB_Balance_TypeUnknown && !AB_Balance_GetValue(extra));
  assert(emit_account_results(account)==0); // unknown empty balance does not lose the valid batch
  AB_ImExporterContext_free(ctx);
}

static void test_invalid_result_amounts(void) {
  // A malformed import must return a normal job error, not kill the worker
  // or invent a zero-valued booking. The parent discards failed job batches.
  begin_stage("transactions");
  AB_IMEXPORTER_ACCOUNTINFO *account=AB_ImExporterAccountInfo_new();
  AB_TRANSACTION *tx=AB_Transaction_new();
  AB_Transaction_SetType(tx,AB_Transaction_TypeStatement);
  AB_Transaction_SetValue(tx,NULL); // fromDb clears the constructor's default
  AB_ImExporterAccountInfo_AddTransaction(account,tx);
  assert(emit_account_results(account)==GWEN_ERROR_BAD_DATA);
  assert(strstr(last_notice,"ohne gültigen Betrag"));
  AB_ImExporterAccountInfo_free(account);

  account=AB_ImExporterAccountInfo_new();
  AB_BALANCE *balance=AB_Balance_new();
  AB_Balance_SetType(balance,AB_Balance_TypeBooked);
  AB_Balance_SetValue(balance,NULL);
  AB_ImExporterAccountInfo_AddBalance(account,balance);
  assert(emit_account_results(account)==GWEN_ERROR_BAD_DATA);
  AB_Balance_SetType(balance,AB_Balance_TypeDisposable);
  assert(emit_account_results(account)==GWEN_ERROR_BAD_DATA);
  AB_ImExporterAccountInfo_free(account);

  BC_FRAME frame=event("test");
  AB_VALUE *zero=AB_Value_fromString("0.00"); AB_Value_SetCurrency(zero,"EUR");
  assert(emit_value(&frame,zero)==0);
  AB_VALUE *decoded=AB_Value_fromString(bc_text(&frame,"amount")); assert(decoded);
  assert(AB_Value_Compare(decoded,zero)==0); AB_Value_free(decoded);
  AB_Value_free(zero); bc_free(&frame);
  char huge[1100]; memset(huge,'9',sizeof(huge)-1); huge[sizeof(huge)-1]=0;
  AB_VALUE *overflow=AB_Value_fromString(huge); assert(overflow);
  frame=event("test"); assert(emit_value(&frame,overflow)==GWEN_ERROR_BAD_DATA);
  assert(!bc_field(&frame,"amount")); AB_Value_free(overflow); bc_free(&frame);
  begin_stage("initialize");
}

static void test_socket_wait_progress(void) {
  FILE *saved=bc_output; FILE *output=tmpfile(); assert(output); bc_output=output;
  assert(progress_start(NULL,0,"Waiting for Data","Waiting for data to become available",0,0)==1);
  assert(ftell(output)==0);
  assert(progress_start(NULL,0,"Banking","Umsätze abrufen",0,0)==1);
  assert(ftell(output)>0); // meaningful progress is still forwarded
  was_cancelled=1; assert(progress_advance(NULL,1,0)==GWEN_ERROR_USER_ABORTED);
  was_cancelled=0; bc_output=saved; fclose(output);
}

static const char *fail_command;
static int setup_calls, setup_failure, setup_cancel, setup_bad_cert;
static char setup_trace[256];
static int fake_control(AB_BANKING *bank,const char *cmd) {
  (void)bank;
  begin_stage(cmd);
  assert(strlen(setup_trace)+strlen(cmd)+2<sizeof(setup_trace));
  strcat(setup_trace,cmd); strcat(setup_trace,";"); setup_calls++;
  if(fail_command && !strcmp(cmd,fail_command)) {
    progress_log(NULL,0,GWEN_LoggerLevel_Error,"HBCI: 9010 - Synthetic original rejection (S)");
    progress_log(NULL,0,GWEN_LoggerLevel_Error,"HBCI: 9999 - Dialog bereits geschlossen. (S)");
    was_cancelled=setup_cancel; certificate_rejected=setup_bad_cert;
    return setup_failure;
  }
  return 0;
}
static int fake_tan(AB_BANKING *bank,int force) {
  assert(!force); return fake_control(bank,"select_tan");
}
static int fake_sepa(AB_BANKING *bank) { return fake_control(bank,"getaccsepa"); }
static void test_snapshot_groups_and_upgrade(AB_BANKING *bank) {
  const char *serialized="settings {\n updjob {\n char job=\"HKKKS\"\n int minSign=1\n }\n"
      " updjob {\n char job=\"HKKKU\"\n int minSign=2\n }\n"
      " tanMethod {\n int function=900\n char methodName=\"First\"\n }\n"
      " tanMethod {\n int function=920\n char methodName=\"BestSign\"\n }\n}\n";
  GWEN_DB_NODE *db=GWEN_DB_Group_new("snapshot");
  assert(GWEN_DB_ReadFromString(db,serialized,strlen(serialized),BC_DB_READ_FLAGS)==0);
  GWEN_DB_NODE *settings=GWEN_DB_GetGroup(db,GWEN_PATH_FLAGS_NAMEMUSTEXIST,"settings");
  GWEN_DB_NODE *job=GWEN_DB_FindFirstGroup(settings,"updjob"); assert(job);
  assert(!strcmp(GWEN_DB_GetCharValue(job,"job",0,""),"HKKKS"));
  assert(GWEN_DB_GetCharValue(job,"job",1,NULL)==NULL);
  job=GWEN_DB_FindNextGroup(job,"updjob"); assert(job);
  assert(!strcmp(GWEN_DB_GetCharValue(job,"job",0,""),"HKKKU"));
  assert(GWEN_DB_GetIntValue(job,"minSign",0,0)==2);
  GWEN_DB_NODE *method=GWEN_DB_FindFirstGroup(settings,"tanMethod"); assert(method);
  method=GWEN_DB_FindNextGroup(method,"tanMethod"); assert(method);
  assert(GWEN_DB_GetIntValue(method,"function",0,0)==920);
  GWEN_DB_Group_free(db);
  AB_PROVIDER *provider=AB_Banking_BeginUseProvider(bank,"aqhbci"); assert(provider);
  AB_USER *user=NULL; assert(AB_Provider_GetUser(provider,(uint32_t)user_id(),1,0,&user)==0);
  AH_User_SetBpdVersion(user,42); AH_User_SetUpdVersion(user,24);
  AH_User_SetTanMediumId(user,"Keep Device");
  assert(AB_Provider_EndExclUseUser(provider,user,0)==0); AB_User_free(user);
  GWEN_DB_SetIntValue(bc_config,GWEN_DB_FLAGS_OVERWRITE_VARS,"initialized",1);
  GWEN_DB_SetIntValue(bc_config,GWEN_DB_FLAGS_OVERWRITE_VARS,"sepa_initialized",1);
  assert(upgrade_snapshot(bank)==0);
  assert(!GWEN_DB_GetIntValue(bc_config,"initialized",0,0));
  assert(!GWEN_DB_GetIntValue(bc_config,"sepa_initialized",0,0));
  assert(GWEN_DB_GetIntValue(bc_config,"snapshot_format",0,0)==2);
  assert(AB_Provider_GetUser(provider,(uint32_t)user_id(),1,1,&user)==0);
  assert(AH_User_GetBpdVersion(user)==0 && AH_User_GetUpdVersion(user)==0);
  assert(!strcmp(AH_User_GetTanMediumId(user),"Keep Device")); AB_User_free(user);
  GWEN_DB_SetIntValue(bc_config,GWEN_DB_FLAGS_OVERWRITE_VARS,"initialized",1);
  assert(upgrade_snapshot(bank)==0 && GWEN_DB_GetIntValue(bc_config,"initialized",0,0)==1);
  assert(AB_Banking_EndUseProvider(bank,provider)==0);
}
static void reset_setup(void) {
  GWEN_DB_DeleteVar(bc_config,"initialized");
  setup_trace[0]=0; setup_calls=0; fail_command=NULL;
  setup_failure=3; setup_cancel=setup_bad_cert=was_cancelled=certificate_rejected=0;
  bc_free(&request); request=event("test"); bc_string(&request,"medium","pintan");
}
static void test_setup_and_errors(void) {
  int fetched=-1;
  reset_setup(); fail_command="getsysid";
  assert(initialize_access(NULL,fake_control,fake_tan,&fetched)==3);
  assert(!fetched && !GWEN_DB_GetIntValue(bc_config,"initialized",0,0));
  assert(!strcmp(setup_trace,"getbankinfo;getsysid;"));
  assert(strstr(last_notice,"9010")<strstr(last_notice,"9999"));
  assert(!strcmp(operation_stage,"getsysid"));
  // Repeated cleanup must not drown out the first error.
  progress_log(NULL,0,GWEN_LoggerLevel_Error,"HBCI: 9999 - Dialog bereits geschlossen. (S)");
  assert(notice_count==2);
  for(unsigned i=0;i<20;i++) {
    char text[40]; snprintf(text,sizeof(text),"Synthetic follow-up %u",i);
    progress_log(NULL,0,GWEN_LoggerLevel_Warning,text);
  }
  assert(notice_count==8 && strstr(last_notice,"9010"));

  // A later, explicit retry may complete setup; the success marker must only
  // appear after all authenticated steps succeed, and survive serialization.
  setup_trace[0]=0; setup_calls=0; fail_command=NULL;
  assert(initialize_access(NULL,fake_control,fake_tan,&fetched)==0 && fetched);
  assert(!strcmp(setup_trace,"getbankinfo;getsysid;select_tan;getaccounts;"));
  GWEN_BUFFER *snapshot=GWEN_Buffer_new(NULL,1024,0,1);
  assert(GWEN_DB_WriteToBuffer(bc_config,snapshot,GWEN_DB_FLAGS_DEFAULT)==0);
  GWEN_DB_NODE *restored=GWEN_DB_Group_new("bankingcraft");
  assert(GWEN_DB_ReadFromString(restored,GWEN_Buffer_GetStart(snapshot),GWEN_Buffer_GetUsedBytes(snapshot),BC_DB_READ_FLAGS)==0);
  assert(GWEN_DB_GetIntValue(restored,"initialized",0,0)==1);
  GWEN_DB_Group_free(restored); GWEN_Buffer_free(snapshot);
  assert(initialize_access(NULL,fake_control,fake_tan,&fetched)==0 && !fetched);
  assert(setup_calls==5); // local TAN validation, no repeated bank login

  reset_setup(); fail_command="getaccounts";
  assert(initialize_access(NULL,fake_control,fake_tan,&fetched)==3 && !fetched);
  assert(!GWEN_DB_GetIntValue(bc_config,"initialized",0,0));
  reset_setup(); fail_command="getbankinfo";
  assert(initialize_access(NULL,fake_control,fake_tan,&fetched)==0 && fetched);
  assert(setup_calls==4); // unsupported anonymous BPD is not fatal
  assert(!*last_notice); // no stale error from the optional operation
  reset_setup(); fail_command="getbankinfo"; setup_bad_cert=1;
  assert(initialize_access(NULL,fake_control,fake_tan,&fetched)==GWEN_ERROR_SSL_SECURITY);
  assert(setup_calls==1 && !fetched);
  reset_setup(); fail_command="getbankinfo"; setup_cancel=1;
  assert(initialize_access(NULL,fake_control,fake_tan,&fetched)==GWEN_ERROR_USER_ABORTED);
  assert(setup_calls==1 && !fetched);
  reset_setup(); bc_free(&request); request=event("test"); bc_string(&request,"medium","ddvcard");
  assert(initialize_access(NULL,fake_control,fake_tan,&fetched)==0 && fetched);
  assert(!strcmp(setup_trace,"select_tan;getaccounts;"));
  reset_setup();
}

static void install_tan_fixture(AB_PROVIDER *provider) {
  AB_USER *user=NULL; assert(AB_Provider_GetUser(provider,(uint32_t)user_id(),1,0,&user)==0);
  GWEN_DB_NODE *base=GWEN_DB_Group_new("user"); assert(AB_User_WriteToDb(user,base)==0);
  const struct {int version;const char *field,*value;int required;} cases[]={
    {3,"tanMediumIdNeeded","J",1},{3,"tanMediumIdNeeded","N",0},
    {6,"needTanMediumId","0",0},{6,"needTanMediumId","1",0},{6,"needTanMediumId","2",1},
    {7,"needTanMediumId","2",1}};
  for(unsigned i=0;i<sizeof(cases)/sizeof(cases[0]);i++) {
    GWEN_DB_NODE *db=GWEN_DB_Group_dup(base); char path[128];
    snprintf(path,sizeof(path),"data/backend/bpd/bpdjobs/300/HITANS/%d/tanMethod",cases[i].version);
    GWEN_DB_NODE *method=GWEN_DB_GetGroup(db,GWEN_DB_FLAGS_DEFAULT,path);
    GWEN_DB_SetCharValue(method,GWEN_DB_FLAGS_OVERWRITE_VARS,cases[i].field,cases[i].value);
    GWEN_DB_SetIntValue(method,GWEN_DB_FLAGS_OVERWRITE_VARS,"function",920);
    GWEN_DB_SetCharValue(method,GWEN_DB_FLAGS_OVERWRITE_VARS,"methodName","Synthetic BestSign");
    assert(AB_User_ReadFromDb(user,db)==0);
    const AH_TAN_METHOD *loaded=AH_TanMethod_List_First(AH_User_GetTanMethodDescriptions(user));
    assert(loaded && AH_TanMethod_GetGvVersion(loaded)==cases[i].version);
    assert(AH_TanMethod_GetNeedTanMediumId(loaded)==cases[i].required);
    GWEN_DB_Group_free(db);
  }
  // Leave the last (v7) fixture selected, just like a partially initialized
  // access whose bank has already supplied methods but still requires a name.
  AH_User_AddTanMethod(user,920); AH_User_SetSelectedTanMethod(user,7920);
  AH_User_SetTanMediumId(user,NULL);
  assert(AB_Provider_EndExclUseUser(provider,user,0)==0);
  AB_User_free(user); GWEN_DB_Group_free(base);
}
static void assert_saved_medium(AB_PROVIDER *provider,const char *expected) {
  AB_USER *user=NULL; assert(AB_Provider_GetUser(provider,(uint32_t)user_id(),1,1,&user)==0);
  assert(!strcmp(str(AH_User_GetTanMediumId(user)),expected)); AB_User_free(user);
}
static void test_tan_medium(AB_BANKING *bank) {
  AB_PROVIDER *provider=AB_Banking_BeginUseProvider(bank,"aqhbci"); assert(provider);
  install_tan_fixture(provider);
  assert(!valid_medium_name("") && !valid_medium_name("   ") && !valid_medium_name("Device\nName"));
  assert(!valid_medium_name("123456789012345678901234567890123"));
  assert(valid_medium_name(" BestSign Gerät "));
  assert(select_tan(bank,0)==0); // prompts for the missing device, not a PIN
  assert_saved_medium(provider," BestSign Gerät ");
  assert(select_tan(bank,0)==0); // saved name is reused without another prompt
  assert(select_tan(bank,1)==0); // method plus replacement device
  assert_saved_medium(provider,"Second Device");
  assert(select_tan(bank,1)==GWEN_ERROR_USER_ABORTED); // cancel device edit
  assert_saved_medium(provider,"Second Device"); // previous config survives
  was_cancelled=0;
  assert(select_tan(bank,1)==0); // overlong name is re-prompted, then 32 Unicode chars
  assert_saved_medium(provider,"ÜÜÜÜÜÜÜÜÜÜÜÜÜÜÜÜÜÜÜÜÜÜÜÜÜÜÜÜÜÜÜÜ");
  AB_USER *user=NULL; assert(AB_Provider_GetUser(provider,(uint32_t)user_id(),1,0,&user)==0);
  AH_User_SetTanMediumId(user,NULL);
  assert(AB_Provider_EndExclUseUser(provider,user,0)==0); AB_User_free(user);
  GWEN_DB_SetIntValue(bc_config,GWEN_DB_FLAGS_OVERWRITE_VARS,"initialized",1);
  int fetched=-1; setup_calls=0;
  assert(initialize_access(bank,fake_control,select_tan,&fetched)==0 && !fetched);
  assert(setup_calls==0); // repair an initialized snapshot before bank I/O
  assert_saved_medium(provider,"Repaired Device");
  assert(AB_Banking_EndUseProvider(bank,provider)==0);
}
static void set_statement_fixture(AB_PROVIDER *provider,int camt) {
  AB_USER *user=NULL; assert(AB_Provider_GetUser(provider,(uint32_t)user_id(),1,0,&user)==0);
  GWEN_DB_NODE *db=GWEN_DB_Group_new("user"); assert(AB_User_WriteToDb(user,db)==0);
  GWEN_DB_NODE *jobs=GWEN_DB_GetGroup(db,GWEN_DB_FLAGS_DEFAULT,"data/backend/bpd/bpdjobs/300");
  GWEN_DB_DeleteGroup(jobs,"HICAZS"); GWEN_DB_DeleteGroup(jobs,"HIKAZS");
  GWEN_DB_NODE *job=GWEN_DB_GetGroup(jobs,GWEN_DB_FLAGS_DEFAULT,camt?"HICAZS/1":"HIKAZS/7");
  GWEN_DB_SetIntValue(job,GWEN_DB_FLAGS_OVERWRITE_VARS,"jobsPerMsg",1);
  GWEN_DB_SetIntValue(job,GWEN_DB_FLAGS_OVERWRITE_VARS,"minSigs",1);
  GWEN_DB_SetIntValue(job,GWEN_DB_FLAGS_OVERWRITE_VARS,"storeDays",90);
  if(camt) GWEN_DB_SetCharValue(job,GWEN_DB_FLAGS_OVERWRITE_VARS,"supportedFormat","urn:iso:std:iso:20022:tech:xsd:camt.052.001.08");
  GWEN_DB_SetCharValue(db,GWEN_DB_FLAGS_OVERWRITE_VARS,"data/backend/upd/1234567890-none/updjob/job",camt?"HKCAZ":"HKKAZ");
  assert(AB_User_ReadFromDb(user,db)==0);
  assert(AB_Provider_EndExclUseUser(provider,user,0)==0);
  AB_User_free(user); GWEN_DB_Group_free(db);
}
static void test_camt_preference(AB_BANKING *bank) {
  AB_PROVIDER *provider=AB_Banking_BeginUseProvider(bank,"aqhbci"); assert(provider);
  set_statement_fixture(provider,1);
  AB_ACCOUNT *account=AB_Provider_CreateAccountObject(provider); assert(account);
  AB_Account_SetUserId(account,(uint32_t)user_id()); AB_Account_SetCountry(account,"de");
  AB_Account_SetBankCode(account,"12345678"); AB_Account_SetAccountNumber(account,"1234567890");
  AB_Account_SetIban(account,"DE02120300000000202051"); AB_Account_SetBic(account,"BYLADEM1001");
  AB_Account_SetCurrency(account,"EUR");
  AH_Account_AddFlags(account,AH_BANK_FLAGS_SEPA_PREFER_SINGLE_TRANSFER);
  assert(AB_Provider_AddAccount(provider,account,1)==0);
  uint32_t id=AB_Account_GetUniqueId(account); AB_Account_free(account);
  AB_ACCOUNT_SPEC *spec=NULL; assert(AB_Banking_GetAccountSpecByUniqueId(bank,id,&spec)==0);
  assert(!AB_AccountSpec_GetTransactionLimitsForCommand(spec,AB_Transaction_CommandGetTransactions));
  AB_AccountSpec_free(spec);
  assert(prefer_camt_accounts(bank)==0);
  assert(AB_Banking_GetAccountSpecByUniqueId(bank,id,&spec)==0);
  assert(AB_AccountSpec_GetTransactionLimitsForCommand(spec,AB_Transaction_CommandGetTransactions));
  AB_AccountSpec_free(spec);
  assert(AB_Provider_GetAccount(provider,id,1,1,&account)==0);
  assert((AH_Account_GetFlags(account)&(AH_BANK_FLAGS_PREFER_CAMT_DOWNLOAD|AH_BANK_FLAGS_SEPA_PREFER_SINGLE_TRANSFER))==
      (AH_BANK_FLAGS_PREFER_CAMT_DOWNLOAD|AH_BANK_FLAGS_SEPA_PREFER_SINGLE_TRANSFER));
  for(int camt=1;camt>=0;camt--) {
    set_statement_fixture(provider,camt);
    AB_USER *user=NULL; assert(AB_Provider_GetUser(provider,(uint32_t)user_id(),1,1,&user)==0);
    AH_JOB *job=NULL;
    assert(AH_Provider_CreateHbciJob(provider,user,account,AB_Transaction_CommandGetTransactions,&job)==0);
    assert(!strcmp(AH_Job_GetName(job),camt?"JobGetTransactionsCAMT":"JobGetTransactions"));
    AH_Job_free(job); AB_User_free(user);
    assert(prefer_camt_accounts(bank)==0); // also rebuild limits for fallback
    assert(AB_Banking_GetAccountSpecByUniqueId(bank,id,&spec)==0);
    assert(AB_AccountSpec_GetTransactionLimitsForCommand(spec,AB_Transaction_CommandGetTransactions));
    AB_AccountSpec_free(spec);
  }
  // CAMT in bank-wide BPD is not permission to use it on every account.
  // Reproduce a savings/card account with no supported statement job while
  // another account at the same bank offers CAMT.
  set_statement_fixture(provider,1);
  AB_USER *user=NULL; assert(AB_Provider_GetUser(provider,(uint32_t)user_id(),1,0,&user)==0);
  GWEN_DB_NODE *db=GWEN_DB_Group_new("user"); assert(AB_User_WriteToDb(user,db)==0);
  GWEN_DB_SetCharValue(db,GWEN_DB_FLAGS_OVERWRITE_VARS,"data/backend/upd/1234567890-none/updjob/job","HKSAL");
  assert(AB_User_ReadFromDb(user,db)==0); GWEN_DB_Group_free(db);
  assert(AB_Provider_EndExclUseUser(provider,user,0)==0); AB_User_free(user);
  assert(prefer_camt_accounts(bank)==0);
  assert(AB_Banking_GetAccountSpecByUniqueId(bank,id,&spec)==0);
  assert(!AB_AccountSpec_GetTransactionLimitsForCommand(spec,AB_Transaction_CommandGetTransactions));
  AB_AccountSpec_free(spec);
  // A credit-card statement permission must select AqBanking's card job even
  // with CAMT available at this bank. Native UID mapping takes precedence over
  // the old account-number group (which deliberately still says CAMT here).
  set_statement_fixture(provider,1);
  assert(AB_Provider_GetUser(provider,(uint32_t)user_id(),1,0,&user)==0);
  db=GWEN_DB_Group_new("user"); assert(AB_User_WriteToDb(user,db)==0);
  GWEN_DB_NODE *card=GWEN_DB_GetGroup(db,GWEN_DB_FLAGS_DEFAULT,"data/backend/bpd/bpdjobs/300/HIKKUS/1");
  GWEN_DB_SetIntValue(card,GWEN_DB_FLAGS_OVERWRITE_VARS,"jobsPerMsg",1);
  GWEN_DB_SetIntValue(card,GWEN_DB_FLAGS_OVERWRITE_VARS,"minSigs",1);
  char path[128]; snprintf(path,sizeof(path),"data/backend/upd/uaid-%08x/updjob/job",id);
  GWEN_DB_SetCharValue(db,GWEN_DB_FLAGS_OVERWRITE_VARS,path,"HKKKU");
  assert(AB_User_ReadFromDb(user,db)==0); GWEN_DB_Group_free(db);
  assert(AB_Provider_EndExclUseUser(provider,user,0)==0); AB_User_free(user);
  assert(prefer_camt_accounts(bank)==0);
  AB_Account_free(account);
  assert(AB_Provider_GetAccount(provider,id,1,1,&account)==0);
  assert(!(AH_Account_GetFlags(account)&AH_BANK_FLAGS_PREFER_CAMT_DOWNLOAD));
  assert(AH_Account_GetFlags(account)&AH_BANK_FLAGS_SEPA_PREFER_SINGLE_TRANSFER);
  assert(AB_Provider_GetUser(provider,(uint32_t)user_id(),1,1,&user)==0);
  AH_JOB *job=NULL;
  assert(AH_Provider_CreateHbciJob(provider,user,account,AB_Transaction_CommandGetTransactions,&job)==0);
  assert(!strcmp(AH_Job_GetName(job),"JobGetTransactionsCreditCard2"));
  AH_Job_free(job); AB_User_free(user);
  assert(AB_Banking_GetAccountSpecByUniqueId(bank,id,&spec)==0);
  assert(AB_AccountSpec_GetTransactionLimitsForCommand(spec,AB_Transaction_CommandGetTransactions));
  AB_AccountSpec_free(spec);
  // Some UPD lists retain a legacy statement permission even when its BPD
  // job has disappeared. Do not stop at that entry and miss a working card job.
  assert(AB_Provider_GetUser(provider,(uint32_t)user_id(),1,0,&user)==0);
  db=GWEN_DB_Group_new("user"); assert(AB_User_WriteToDb(user,db)==0);
  snprintf(path,sizeof(path),"data/backend/upd/uaid-%08x",id);
  GWEN_DB_NODE *upd=GWEN_DB_GetGroup(db,GWEN_DB_FLAGS_DEFAULT,path);
  GWEN_DB_SetCharValue(upd,GWEN_DB_FLAGS_OVERWRITE_VARS,"updjob/job","HKKAZ");
  GWEN_DB_NODE *supported=GWEN_DB_Group_new("updjob");
  GWEN_DB_SetCharValue(supported,GWEN_DB_FLAGS_DEFAULT,"job","HKKKU");
  GWEN_DB_AddGroup(upd,supported);
  assert(AB_User_ReadFromDb(user,db)==0); GWEN_DB_Group_free(db);
  assert(AB_Provider_EndExclUseUser(provider,user,0)==0); AB_User_free(user);
  assert(prefer_camt_accounts(bank)==0);
  assert(AB_Banking_GetAccountSpecByUniqueId(bank,id,&spec)==0);
  assert(AB_AccountSpec_GetTransactionLimitsForCommand(spec,AB_Transaction_CommandGetTransactions));
  AB_AccountSpec_free(spec);
  assert(AB_Provider_GetUser(provider,(uint32_t)user_id(),1,1,&user)==0);
  job=NULL;
  assert(AH_Provider_CreateHbciJob(provider,user,account,AB_Transaction_CommandGetTransactions,&job)==0);
  assert(!strcmp(AH_Job_GetName(job),"JobGetTransactionsCreditCard2"));
  AH_Job_free(job); AB_User_free(user);
  AB_Account_free(account); assert(AB_Banking_EndUseProvider(bank,provider)==0);
}

static void test_sepa_accounts(AB_BANKING *bank) {
  // No HISPAS in the synthetic bank's BPD: skip without opening any connection.
  assert(get_sepa_accounts(bank)==0);
  GWEN_DB_DeleteVar(bc_config,"sepa_initialized");
  setup_trace[0]=0; setup_calls=0; fail_command=NULL;
  assert(ensure_sepa_accounts(bank,0,fake_sepa)==0 && setup_calls==1);
  assert(ensure_sepa_accounts(bank,0,fake_sepa)==0 && setup_calls==1);
  assert(ensure_sepa_accounts(bank,1,fake_sepa)==0 && setup_calls==2);
  fail_command="getaccsepa"; setup_failure=GWEN_ERROR_GENERIC;
  assert(ensure_sepa_accounts(bank,1,fake_sepa)==GWEN_ERROR_GENERIC);
  assert(!GWEN_DB_GetIntValue(bc_config,"sepa_initialized",0,0));
  assert(!strcmp(operation_stage,"getaccsepa") && strstr(last_notice,"9010"));
  fail_command=NULL;
  assert(ensure_sepa_accounts(bank,0,fake_sepa)==0 && setup_calls==4);
  GWEN_BUFFER *snapshot=GWEN_Buffer_new(NULL,1024,0,1);
  assert(GWEN_DB_WriteToBuffer(bc_config,snapshot,GWEN_DB_FLAGS_DEFAULT)==0);
  GWEN_DB_NODE *restored=GWEN_DB_Group_new("bankingcraft");
  assert(GWEN_DB_ReadFromString(restored,GWEN_Buffer_GetStart(snapshot),GWEN_Buffer_GetUsedBytes(snapshot),BC_DB_READ_FLAGS)==0);
  assert(GWEN_DB_GetIntValue(restored,"sepa_initialized",0,0)==1);
  GWEN_DB_Group_free(restored); GWEN_Buffer_free(snapshot);

  AB_PROVIDER *provider=AB_Banking_BeginUseProvider(bank,"aqhbci"); assert(provider);
  uint32_t ids[2];
  for(int i=0;i<2;i++) {
    AB_ACCOUNT *account=AB_Provider_CreateAccountObject(provider); assert(account);
    AB_Account_SetUserId(account,(uint32_t)user_id()); AB_Account_SetCountry(account,"de");
    AB_Account_SetBankCode(account,"12345678"); AB_Account_SetAccountNumber(account,"0001234567");
    AB_Account_SetSubAccountId(account,i?"02":"01"); AB_Account_SetCurrency(account,"EUR");
    assert(AB_Provider_AddAccount(provider,account,1)==0);
    ids[i]=AB_Account_GetUniqueId(account); AB_Account_free(account);
  }
  AB_USER *user=NULL; assert(AB_Provider_GetUser(provider,(uint32_t)user_id(),1,0,&user)==0);
  GWEN_DB_NODE *db=GWEN_DB_Group_new("user"); assert(AB_User_WriteToDb(user,db)==0);
  char path[128]; snprintf(path,sizeof(path),"data/backend/upd/uaid-%08x/updjob/job",ids[0]);
  GWEN_DB_SetCharValue(db,GWEN_DB_FLAGS_OVERWRITE_VARS,path,"HKCAZ");
  assert(AB_User_ReadFromDb(user,db)==0); GWEN_DB_Group_free(db);
  // Exercise AqBanking's actual HISPA importer, including its matching of
  // national account + suffix to the stored UID. No application-side parsing.
  GWEN_DB_NODE *responses=GWEN_DB_Group_new("responses");
  GWEN_DB_NODE *sepa=GWEN_DB_GetGroup(responses,GWEN_DB_FLAGS_DEFAULT,
      "GetAccountSepaInfoResponse/data/GetAccountSepaInfoResponse/account");
  GWEN_DB_SetCharValue(sepa,GWEN_DB_FLAGS_DEFAULT,"bankCode","12345678");
  GWEN_DB_SetCharValue(sepa,GWEN_DB_FLAGS_DEFAULT,"accountId","0001234567");
  GWEN_DB_SetCharValue(sepa,GWEN_DB_FLAGS_DEFAULT,"accountsubid","01");
  GWEN_DB_SetCharValue(sepa,GWEN_DB_FLAGS_DEFAULT,"iban","DE89370400440532013000");
  GWEN_DB_SetCharValue(sepa,GWEN_DB_FLAGS_DEFAULT,"bic","COBADEFFXXX");
  GWEN_DB_SetCharValue(sepa,GWEN_DB_FLAGS_DEFAULT,"sepa","J");
  AH_JOBQUEUE *queue=AH_JobQueue_new(user);
  AH_JobQueue_ReadSepaAccounts(queue,responses);
  AH_JobQueue_free(queue); GWEN_DB_Group_free(responses);
  assert(AB_Provider_EndExclUseUser(provider,user,0)==0); AB_User_free(user);
  assert(prefer_camt_accounts(bank)==0);
  AB_ACCOUNT_SPEC *spec=NULL;
  assert(AB_Banking_GetAccountSpecByUniqueId(bank,ids[0],&spec)==0);
  assert(!strcmp(str(AB_AccountSpec_GetIban(spec)),"DE89370400440532013000"));
  assert(!strcmp(str(AB_AccountSpec_GetBic(spec)),"COBADEFFXXX"));
  assert(!strcmp(str(AB_AccountSpec_GetAccountNumber(spec)),"0001234567"));
  assert(!strcmp(str(AB_AccountSpec_GetSubAccountNumber(spec)),"01"));
  AB_AccountSpec_free(spec);
  assert(AB_Banking_GetAccountSpecByUniqueId(bank,ids[1],&spec)==0);
  assert(!*str(AB_AccountSpec_GetIban(spec))); AB_AccountSpec_free(spec);
  AB_ACCOUNT *account=NULL; assert(AB_Provider_GetAccount(provider,ids[0],1,1,&account)==0);
  assert(AB_Provider_GetUser(provider,(uint32_t)user_id(),1,1,&user)==0);
  AH_JOB *job=NULL;
  assert(AH_Provider_CreateHbciJob(provider,user,account,AB_Transaction_CommandGetTransactions,&job)==0);
  assert(!strcmp(AH_Job_GetName(job),"JobGetTransactionsCAMT"));
  GWEN_DB_NODE *args=AH_Job_GetArguments(job);
  assert(!strcmp(GWEN_DB_GetCharValue(args,"iban",0,""),"DE89370400440532013000"));
  assert(!strcmp(GWEN_DB_GetCharValue(args,"bic",0,""),"COBADEFFXXX"));
  assert(!strcmp(GWEN_DB_GetCharValue(args,"accountId",0,""),"0001234567"));
  assert(!strcmp(GWEN_DB_GetCharValue(args,"accountSubId",0,""),"01"));
  AH_Job_free(job); AB_Account_free(account); AB_User_free(user);
  assert(AB_Banking_EndUseProvider(bank,provider)==0);
}

int main(int argc,char **argv) {
  assert(argc==2);
  bc_output=stdout;
  assert(GWEN_Init()==0);
  GWEN_GUI *gui=setup_gui();
  bc_config=GWEN_DB_Group_new("bankingcraft");
  assert(bc_install_memory_config()==0);
  AB_BANKING *bank=AB_Banking_new("BankingCraftTest",argv[1],0);
  assert(AB_Banking_Init(bank)==0);
  test_camt_entry_amounts(bank);
  test_invalid_result_amounts();
  test_socket_wait_progress();
  test_automatic_card_context(bank);
  test_card_read_session(bank);
  GWEN_PLUGIN_MANAGER *tokens=GWEN_PluginManager_FindPluginManager("ct"); assert(tokens);
  const char *token_names[]={"ddvcard","zkacard"};
  for(unsigned i=0;i<2;i++) {
    GWEN_PLUGIN *plugin=GWEN_PluginManager_GetPlugin(tokens,token_names[i]); assert(plugin);
    GWEN_CRYPT_TOKEN *token=GWEN_Crypt_Token_Plugin_CreateToken(plugin,NULL); assert(token);
    GWEN_Crypt_Token_free(token); // no card open, PIN prompt or device I/O
  }
  request=event("configure");
  bc_string(&request,"blz","12345678"); bc_string(&request,"user","synthetic-user");
  bc_string(&request,"customer","synthetic-user"); bc_string(&request,"medium","pintan");
  bc_string(&request,"url","https://bank.invalid"); bc_string(&request,"record","1");
  assert(ensure_user(bank)==0);
  AB_PROVIDER *provider=AB_Banking_BeginUseProvider(bank,"aqhbci"); assert(provider);
  AB_USER *user=NULL; assert(user_id()>0);
  assert(AB_Provider_GetUser(provider,(uint32_t)user_id(),1,0,&user)==0);
  AH_DIALOG *hbci=AH_Dialog_new(user,provider); assert(hbci);
  assert(AH_Dialog_GetLogFile(hbci)==NULL); AH_Dialog_free(hbci);
  assert(AB_Provider_EndExclUseUser(provider,user,0)==0); AB_User_free(user);
  assert(AB_Banking_EndUseProvider(bank,provider)==0); bc_free(&request);
  test_snapshot_groups_and_upgrade(bank); test_setup_and_errors();
  test_tan_medium(bank);
  test_camt_preference(bank); test_sepa_accounts(bank); bc_free(&request);
  GWEN_SSLCERTDESCR *cert=GWEN_SslCertDescr_new();
  GWEN_SslCertDescr_SetIsError(cert,1);
  assert(check_certificate(gui,cert,NULL,0)<0);
  GWEN_SslCertDescr_free(cert);
  GWEN_DIALOG *dialog=AH_VopDialog_new("Synthetic transfer","Synthetic bank","Synthetic user",
      "SYNTHETIC FULL BANK NOTICE\nRecipient differs: Alice / Corrected Alice\nDE02120300000000202051",NULL);
  assert(dialog);
  int result=execute_dialog(gui,dialog,0);
  GWEN_Dialog_free(dialog);
  assert(AB_Banking_Fini(bank)==0); AB_Banking_free(bank);
  GWEN_DB_SetIntValue(bc_config,GWEN_DB_FLAGS_OVERWRITE_VARS,"initialized",1);
  GWEN_DB_SetIntValue(bc_config,GWEN_DB_FLAGS_OVERWRITE_VARS,"sepa_initialized",1);
  BC_FRAME ready=event("ready"); emit_db(&ready,"state",bc_config); bc_send(&ready);
  // Synthetic, partially initialized state for the process-level repair test.
  GWEN_DB_SetIntValue(bc_config,GWEN_DB_FLAGS_OVERWRITE_VARS,"initialized",0);
  BC_FRAME f=event("result");bc_number(&f,"accepted",result);emit_db(&f,"state",bc_config);bc_send(&f);
  GWEN_DB_Group_free(bc_config);
  GWEN_Gui_SetGui(NULL);GWEN_Gui_free(gui);GWEN_Fini();return 0;
}
