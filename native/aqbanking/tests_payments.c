/* SPDX-License-Identifier: GPL-3.0-only
 * Real AqBanking factory/parser/exporter/result handlers, synthetic BPD/UPD.
 * This executable cannot open a network connection, even accidentally. */
#define AB_Banking_SendCommands payment_test_send
#define main bankingcraft_unused_worker_main
#include "main.c"
#undef main
#undef AB_Banking_SendCommands
#include <aqhbci/banking/provider_job.h>
#include <aqhbci/joblayer/job_l.h>
#include <aqhbci/joblayer/result_l.h>
#include <aqhbci/msglayer/dialog_l.h>
#include <aqbanking/banking_imex.h>
#include <sys/socket.h>
#include <assert.h>
int connect(int fd,const struct sockaddr *address,socklen_t length) {
  (void)fd;(void)address;(void)length; abort();
}
static int dispatch_calls,dispatch_expected,dispatch_result;
int payment_test_send(AB_BANKING *bank,AB_TRANSACTION_LIST2 *commands,AB_IMEXPORTER_CONTEXT *ctx) {
  (void)bank;(void)ctx;
  AB_TRANSACTION_LIST2_ITERATOR *it=AB_Transaction_List2_First(commands);assert(it);
  AB_TRANSACTION *t=AB_Transaction_List2Iterator_Data(it);assert(AB_Transaction_GetCommand(t)==dispatch_expected);
  assert(!AB_Transaction_List2Iterator_Next(it));AB_Transaction_List2Iterator_free(it);dispatch_calls++;
  if(dispatch_expected==AB_Transaction_CommandGetSepaInstantStatus) {
    assert(!strcmp(str(AB_Transaction_GetFiId(t)),"SYNTHETIC-REF"));
    assert(!AB_Transaction_GetValue(t) || AB_Value_IsZero(AB_Transaction_GetValue(t)));
    assert(!*str(AB_Transaction_GetRemoteIban(t)));
  } else {
    assert(AB_Transaction_GetValue(t));assert(!strcmp(str(AB_Transaction_GetEndToEndReference(t)),"IPC-E2E"));
    assert((AB_Transaction_GetDate(t)!=NULL)==(dispatch_expected==AB_Transaction_CommandSepaCreateDatedTransfer));
    assert((AB_Transaction_GetSubType(t)==AB_Transaction_SubTypeEuASAP)==(dispatch_expected==AB_Transaction_CommandSepaInstantTransfer));
  }
  AB_Transaction_SetStatus(t,AB_Transaction_StatusAccepted);
  if(dispatch_expected==AB_Transaction_CommandSepaInstantTransfer || dispatch_expected==AB_Transaction_CommandGetSepaInstantStatus) {
    AB_Transaction_SetFiId(t,"SYNTHETIC-REF");AB_Transaction_SetMemo(t,"executed");AB_Transaction_SetTextKey(t,2);
  }
  return dispatch_result;
}
static void adapter_dispatch(AB_BANKING *bank,uint32_t account) {
  const int commands[]={AB_Transaction_CommandSepaTransfer,AB_Transaction_CommandSepaInstantTransfer,
    AB_Transaction_CommandSepaCreateDatedTransfer,AB_Transaction_CommandGetSepaInstantStatus};
  for(unsigned i=0;i<4;i++) {
    bc_free(&request);request=event("transfer");bc_number(&request,"account",account);
    bc_string(&request,"name","Synthetic Recipient");bc_string(&request,"iban","DE02120300000000202051");
    bc_string(&request,"bic","BYLADEM1001");bc_string(&request,"amount","1.23");bc_string(&request,"purpose","Offline only");
    bc_string(&request,"end_to_end","IPC-E2E");bc_string(&request,"instant",i==1?"1":"0");bc_string(&request,"reference","SYNTHETIC-REF");
    if(i==2) {GWEN_DATE *date=GWEN_Date_CurrentDate();GWEN_Date_AddDays(date,5);GWEN_BUFFER *b=GWEN_Buffer_new(NULL,32,0,1);
      GWEN_Date_toStringWithTemplate(date,"YYYYMMDD",b);bc_string(&request,"date",GWEN_Buffer_GetStart(b));GWEN_Buffer_free(b);GWEN_Date_free(date);}
    dispatch_expected=commands[i];dispatch_result=0;assert(run_job(bank,commands[i])==0);
  }
  assert(dispatch_calls==4);
  bc_free(&request);request=event("transfer");bc_string(&request,"instant","1");bc_number(&request,"account",account);
  assert(run_job(bank,AB_Transaction_CommandSepaTransfer)<0); // the old subtype-only path is forbidden
  bc_string(&request,"date","20990101");assert(run_job(bank,AB_Transaction_CommandSepaInstantTransfer)<0);
  assert(dispatch_calls==4);
}
static const char *debtor="DE89370400440532013000";
static GWEN_DB_NODE *parsed(GWEN_MSGENGINE *engine,const char *wire) {
  GWEN_BUFFER *b=GWEN_Buffer_new(NULL,256,0,1); GWEN_Buffer_AppendString(b,wire); GWEN_Buffer_Rewind(b);
  GWEN_DB_NODE *db=GWEN_DB_Group_new("data");
  assert(GWEN_MsgEngine_ReadMessage(engine,"SEG",b,db,0)==0); GWEN_Buffer_free(b);
  return db;
}
static void params(GWEN_MSGENGINE *engine,GWEN_DB_NODE *jobs,const char *path,const char *wire) {
  GWEN_DB_NODE *db=parsed(engine,wire),*data=GWEN_DB_GetFirstGroup(db); assert(data);
  GWEN_DB_NODE *to=GWEN_DB_GetGroup(jobs,GWEN_DB_FLAGS_DEFAULT,path);
  GWEN_DB_AddGroupChildren(to,data); GWEN_DB_Group_free(db);
}
static void configure(AB_USER *user,AB_ACCOUNT *account,GWEN_MSGENGINE *engine,int version,const char *format,const char *omit,int convert) {
  GWEN_DB_NODE *db=GWEN_DB_Group_new("user"); assert(AB_User_WriteToDb(user,db)==0);
  GWEN_DB_DeleteGroup(db,"data/backend/bpd/bpdjobs/300");
  GWEN_DB_DeleteGroup(db,"data/backend/upd/1234567890-none");
  GWEN_DB_NODE *bpd=GWEN_DB_GetGroup(db,GWEN_DB_FLAGS_DEFAULT,"data/backend/bpd/bpdjobs/300");
  char wire[1024],path[80];
  snprintf(wire,sizeof(wire),"HIIPZS:4:%d:3+1+1+0+%s:urn?:iso?:std?:iso?:20022?:tech?:xsd?:%s'",version,version==2?(convert?"J:":"N:"):"",format);
  snprintf(path,sizeof(path),"HIIPZS/%d",version);
  if(strcmp(omit,"HIIPZS")) params(engine,bpd,path,wire);
  snprintf(wire,sizeof(wire),"HIIPSS:5:1:3+1+1+0+2:urn?:iso?:std?:iso?:20022?:tech?:xsd?:%s'",format);
  if(strcmp(omit,"HIIPSS")) params(engine,bpd,"HIIPSS/1",wire);
  params(engine,bpd,"HICSES/1","HICSES:6:1:3+1+1+0+2:30'");
  params(engine,bpd,"HICCSS/1","HICCSS:7:1:3+1+1+0'");
  GWEN_DB_SetIntValue(bpd,GWEN_DB_FLAGS_OVERWRITE_VARS,"HIVPPS/1/jobsPerMsg",1);
  if(strcmp(omit,"HIVPAS")) GWEN_DB_SetIntValue(bpd,GWEN_DB_FLAGS_OVERWRITE_VARS,"HIVPAS/1/jobsPerMsg",1);
  if(strcmp(omit,"VOP")) {
    GWEN_DB_SetCharValue(bpd,GWEN_DB_FLAGS_DEFAULT,"HIVPPS/1/VopRequiredForJobs/Job","HKIPZ");
    GWEN_DB_SetCharValue(bpd,GWEN_DB_FLAGS_DEFAULT,"HIVPPS/1/VopRequiredForJobs/Job","HKCSE");
  }
  const char *codes[]={"HKIPZ","HKIPS","HKCSE","HKCCS"};
  GWEN_DB_NODE *upd=GWEN_DB_GetGroup(db,GWEN_DB_FLAGS_DEFAULT,"data/backend/upd/1234567890-none");
  for(unsigned i=0;i<4;i++) {
    snprintf(path,sizeof(path),"%s/needTan",codes[i]);
    if(strcmp(omit,"HIPINS")) GWEN_DB_SetIntValue(bpd,GWEN_DB_FLAGS_OVERWRITE_VARS,path,i==1?0:1);
    if(!strcmp(omit,codes[i])) continue;
    GWEN_DB_NODE *permission=GWEN_DB_Group_new("updjob");
    GWEN_DB_SetCharValue(permission,GWEN_DB_FLAGS_DEFAULT,"job",codes[i]);
    GWEN_DB_SetIntValue(permission,GWEN_DB_FLAGS_DEFAULT,"minSign",1); GWEN_DB_AddGroup(upd,permission);
  }
  snprintf(wire,sizeof(wire),"urn:iso:std:iso:20022:tech:xsd:%s",format);
  GWEN_DB_SetCharValue(db,GWEN_DB_FLAGS_OVERWRITE_VARS,"data/backend/bpd/bpdjobs/300/HISPAS/1/SupportedSepaFormats/format",wire);
  assert(AB_User_ReadFromDb(user,db)==0); GWEN_DB_Group_free(db);
  AH_Account_AddFlags(account,AH_BANK_FLAGS_SEPA_PREFER_SINGLE_TRANSFER);
}
static AB_TRANSACTION *transfer(int cmd,int days) {
  AB_TRANSACTION *t=AB_Transaction_new(); AB_Transaction_SetCommand(t,cmd);
  AB_Transaction_SetType(t,AB_Transaction_TypeTransfer);
  AB_Transaction_SetSubType(t,cmd==AB_Transaction_CommandSepaInstantTransfer?AB_Transaction_SubTypeEuASAP:AB_Transaction_SubTypeStandard);
  AB_Transaction_SetLocalName(t,"Synthetic Sender"); AB_Transaction_SetLocalIban(t,debtor); AB_Transaction_SetLocalBic(t,"COBADEFFXXX");
  AB_Transaction_SetRemoteName(t,"Synthetic Recipient"); AB_Transaction_SetRemoteIban(t,"DE02120300000000202051"); AB_Transaction_SetRemoteBic(t,"BYLADEM1001");
  AB_Transaction_SetEndToEndReference(t,"OFFLINE-ONLY-123"); AB_Transaction_SetPurpose(t,"Offline test only");
  AB_VALUE *value=AB_Value_fromString("1.23:EUR"); AB_Transaction_SetValue(t,value); AB_Value_free(value);
  if(days>=0) { GWEN_DATE *date=GWEN_Date_CurrentDate(); GWEN_Date_AddDays(date,days); AB_Transaction_SetDate(t,date); GWEN_Date_free(date); }
  return t;
}
static AH_JOB *job(AB_PROVIDER *provider,AB_USER *user,AB_ACCOUNT *account,int cmd) {
  AH_JOB *j=NULL; assert(AH_Provider_CreateHbciJob(provider,user,account,cmd,&j)==0); assert(j); return j;
}
static void assert_wire(AH_JOB *j,GWEN_MSGENGINE *engine,const char *code,int version) {
  GWEN_DB_NODE *args=AH_Job_GetArguments(j);
  GWEN_DB_SetIntValue(args,GWEN_DB_FLAGS_OVERWRITE_VARS,"head/seq",4);
  GWEN_XMLNODE *node=GWEN_MsgEngine_FindNodeByProperty(engine,"SEG","id",version,
      !strcmp(code,"HKIPZ")?"BCInstantTransfer":!strcmp(code,"HKIPS")?"BCInstantStatus":"BCScheduledTransfer"); if(!node) fprintf(stderr,"Missing node %s v%d protocol%d\n",code,version,GWEN_MsgEngine_GetProtocolVersion(engine)); assert(node);
  GWEN_BUFFER *wire=GWEN_Buffer_new(NULL,2048,0,1);
  assert(GWEN_MsgEngine_CreateMessageFromNode(engine,node,wire,args)==0);
  char prefix[32]; snprintf(prefix,sizeof(prefix),"%s:4:%d+",code,version);
  assert(!strncmp(GWEN_Buffer_GetStart(wire),prefix,strlen(prefix)));
  if(!strcmp(code,"HKIPS")) assert(strstr(GWEN_Buffer_GetStart(wire),"+SYNTHETIC-REF'") && !strstr(GWEN_Buffer_GetStart(wire),"@"));
  if(!strcmp(code,"HKIPZ") && version==2) {
    int flag=*GWEN_DB_GetCharValue(args,"convertToSepa",0,"");
    const char *s=GWEN_Buffer_GetStart(wire);size_t n=GWEN_Buffer_GetUsedBytes(wire);
    assert(flag?strstr(s+n-4,"+N'")!=NULL:s[n-2]!='N');
  }
  GWEN_Buffer_free(wire);
}
static void response(AH_JOB *j,GWEN_MSGENGINE *engine,const char *wire,int code,int msgcode) {
  GWEN_DB_NODE *r=GWEN_DB_Group_new("response");
  if(wire) {GWEN_DB_NODE *db=parsed(engine,wire);GWEN_DB_AddGroup(r,db);}
  if(code) GWEN_DB_SetIntValue(r,GWEN_DB_FLAGS_DEFAULT,"data/SegResult/result/resultcode",code);
  AH_Job_AddResponse(j,r);
  if(msgcode) {
    r=GWEN_DB_Group_new("response");GWEN_DB_SetIntValue(r,GWEN_DB_FLAGS_DEFAULT,"data/MsgResult/result/resultcode",msgcode);
    AH_Job_AddResponse(j,r);
  }
}
static void results(AB_PROVIDER *provider,AB_USER *user,AB_ACCOUNT *account,GWEN_MSGENGINE *engine) {
  for(int status_job=0;status_job<2;status_job++) for(int state=-1;state<=9;state++) {
    int cmd=status_job?AB_Transaction_CommandGetSepaInstantStatus:AB_Transaction_CommandSepaInstantTransfer;
    AH_JOB *j=job(provider,user,account,cmd); AB_TRANSACTION *t=transfer(cmd,-1);
    if(status_job) AB_Transaction_SetFiId(t,"SYNTHETIC-REF");
    assert(AH_Job_HandleCommand(j,t)==0); AH_Job_AddCommand(j,t);
    char wire[512];
    if(status_job) snprintf(wire,sizeof(wire),"HIIPS:5:1:4+%s:COBADEFFXXX+++SYNTHETIC-REF++%d'",debtor,state);
    // Build the two layouts explicitly to keep varargs types correct.
    if(!status_job) snprintf(wire,sizeof(wire),"HIIPZ:5:2:4+SYNTHETIC-REF++%d'",state);
    if(state==0) snprintf(wire,sizeof(wire),status_job?"HIIPS:5:1:4+%s:COBADEFFXXX+++SYNTHETIC-REF'":"HIIPZ:5:2:4+SYNTHETIC-REF'",debtor);
    response(j,engine,state<0?NULL:wire,20,0);
    assert(AH_Job_Process(j,NULL)==0); assert(AH_Job_HandleResults(j,NULL)==0);
    const char *expected=state<0?"unknown":state==0||state==7?"executed":state==2||state==5||state==6||state==8||state==9?"rejected":"pending";
    assert(!strcmp(str(AB_Transaction_GetMemo(t)),expected));
    assert(AB_Transaction_GetTextKey(t)==2);
    if(state>=0) assert(!strcmp(str(AB_Transaction_GetFiId(t)),"SYNTHETIC-REF"));
    AH_Job_free(j); AB_Transaction_free(t);
  }
  const struct {int code,msgcode,state; const char *expected;} cases[]={
    {3045,0,0,"pending"},{3046,0,0,"unknown"},{3270,0,7,"unknown"},{3001,0,7,"unknown"},
    {20,9050,7,"unknown"},{0,20,0,"pending"},{10,0,0,"pending"}};
  for(unsigned i=0;i<sizeof(cases)/sizeof(*cases);i++) {
    AH_JOB *j=job(provider,user,account,AB_Transaction_CommandSepaInstantTransfer);
    AB_TRANSACTION *t=transfer(AB_Transaction_CommandSepaInstantTransfer,-1);
    assert(AH_Job_HandleCommand(j,t)==0);AH_Job_AddCommand(j,t);
    char wire[128];snprintf(wire,sizeof(wire),"HIIPZ:5:2:4+SYNTHETIC-REF++%d'",cases[i].state);
    response(j,engine,wire,cases[i].code,cases[i].msgcode);
    assert(AH_Job_Process(j,NULL)==0);assert(AH_Job_HandleResults(j,NULL)==0);
    if(strcmp(str(AB_Transaction_GetMemo(t)),cases[i].expected)) fprintf(stderr,"Case %u: code%d msg%d state%d -> %s, expected %s\n",i,cases[i].code,cases[i].msgcode,cases[i].state,str(AB_Transaction_GetMemo(t)),cases[i].expected);
    assert(!strcmp(str(AB_Transaction_GetMemo(t)),cases[i].expected));
    AH_Job_free(j);AB_Transaction_free(t);
  }
}
static void boundaries(AB_PROVIDER *provider,AB_USER *user,AB_ACCOUNT *account,GWEN_MSGENGINE *engine) {
  for(int days=-1;days<=31;days++) {
    AH_JOB *j=job(provider,user,account,AB_Transaction_CommandSepaCreateDatedTransfer);
    AB_TRANSACTION *t=transfer(AB_Transaction_CommandSepaCreateDatedTransfer,days);
    assert((AH_Job_HandleCommand(j,t)==0)==(days>=2 && days<=30));
    AH_Job_free(j);AB_Transaction_free(t);
  }
  for(int days=0;days<=1;days++) {
    AH_JOB *j=job(provider,user,account,AB_Transaction_CommandSepaInstantTransfer);
    AB_TRANSACTION *t=transfer(AB_Transaction_CommandSepaInstantTransfer,days);
    assert(AH_Job_HandleCommand(j,t)<0);AH_Job_free(j);AB_Transaction_free(t);
  }
  AH_JOB *j=job(provider,user,account,AB_Transaction_CommandGetSepaInstantStatus);
  AB_TRANSACTION *t=transfer(AB_Transaction_CommandGetSepaInstantStatus,-1);
  assert(AH_Job_HandleCommand(j,t)<0);AB_Transaction_SetFiId(t,"SYNTHETIC-REF");
  assert(AH_Job_HandleCommand(j,t)==0);assert_wire(j,engine,"HKIPS",1);
  AH_Job_free(j);AB_Transaction_free(t);
  // A status from another order/account, or conflicting replies, is never success.
  const char *bad[]={"HIIPS:5:1:4+DE89370400440532013000:COBADEFFXXX+++OTHER-REF++7'",
    "HIIPS:5:1:4+DE02120300000000202051:BYLADEM1001+++SYNTHETIC-REF++7'"};
  for(unsigned i=0;i<2;i++) {
    j=job(provider,user,account,AB_Transaction_CommandGetSepaInstantStatus);t=transfer(AB_Transaction_CommandGetSepaInstantStatus,-1);
    AB_Transaction_SetFiId(t,"SYNTHETIC-REF");assert(AH_Job_HandleCommand(j,t)==0);AH_Job_AddCommand(j,t);
    response(j,engine,bad[i],20,0);assert(AH_Job_Process(j,NULL)<0);assert(AH_Job_HandleResults(j,NULL)==0);
    assert(!strcmp(str(AB_Transaction_GetMemo(t)),"unknown"));AH_Job_free(j);AB_Transaction_free(t);
  }
  // A failed status query is not evidence that the original payment failed.
  j=job(provider,user,account,AB_Transaction_CommandGetSepaInstantStatus);t=transfer(AB_Transaction_CommandGetSepaInstantStatus,-1);
  AB_Transaction_SetFiId(t,"SYNTHETIC-REF");assert(AH_Job_HandleCommand(j,t)==0);AH_Job_AddCommand(j,t);
  response(j,engine,NULL,9210,0);assert(AH_Job_Process(j,NULL)==0);assert(AH_Job_HandleResults(j,NULL)==0);
  assert(!strcmp(str(AB_Transaction_GetMemo(t)),"unknown"));assert(!strcmp(str(AB_Transaction_GetFiId(t)),"SYNTHETIC-REF"));
  AH_Job_free(j);AB_Transaction_free(t);
  const struct {int code,msg;int accepted;} scheduled[]={
    {10,0,1},{20,0,1},{3030,0,0},{0,20,0},{9210,0,0},{20,9050,0},{0,0,0}};
  for(unsigned i=0;i<sizeof(scheduled)/sizeof(*scheduled);i++) {
    j=job(provider,user,account,AB_Transaction_CommandSepaCreateDatedTransfer);t=transfer(AB_Transaction_CommandSepaCreateDatedTransfer,5);
    assert(AH_Job_HandleCommand(j,t)==0);AH_Job_AddCommand(j,t);
    response(j,engine,"HICSE:5:1:4+SYNTHETIC-SCHEDULED'",scheduled[i].code,scheduled[i].msg);
    assert(AH_Job_Process(j,NULL)==0);assert(AH_Job_HandleResults(j,NULL)==0);
    assert((AB_Transaction_GetStatus(t)==AB_Transaction_StatusAccepted)==scheduled[i].accepted);
    assert(!strcmp(str(AB_Transaction_GetFiId(t)),"SYNTHETIC-SCHEDULED"));
    AH_Job_free(j);AB_Transaction_free(t);
  }
  // HICSE's reference is optional, acceptance code 0010 remains authoritative.
  j=job(provider,user,account,AB_Transaction_CommandSepaCreateDatedTransfer);t=transfer(AB_Transaction_CommandSepaCreateDatedTransfer,5);
  assert(AH_Job_HandleCommand(j,t)==0);AH_Job_AddCommand(j,t);response(j,engine,"HICSE:5:1:4'",10,0);
  assert(AH_Job_Process(j,NULL)==0);assert(AH_Job_HandleResults(j,NULL)==0);assert(AB_Transaction_GetStatus(t)==AB_Transaction_StatusAccepted);
  AH_Job_free(j);AB_Transaction_free(t);
}
int main(int argc,char **argv) {
  assert(argc==3);bc_output=tmpfile();assert(bc_output);assert(GWEN_Init()==0);GWEN_GUI *gui=setup_gui();
  bc_config=GWEN_DB_Group_new("bankingcraft");assert(bc_install_memory_config()==0);
  AB_BANKING *bank=AB_Banking_new("BankingCraftPaymentTests",argv[1],0);assert(AB_Banking_Init(bank)==0);
  request=event("configure");bc_string(&request,"blz","12345678");bc_string(&request,"user","synthetic-user");
  bc_string(&request,"customer","synthetic-user");bc_string(&request,"medium","pintan");bc_string(&request,"url","https://bank.invalid");bc_string(&request,"record","1");assert(ensure_user(bank)==0);
  AB_PROVIDER *provider=AB_Banking_BeginUseProvider(bank,"aqhbci");assert(provider);
  AB_USER *user=NULL;assert(AB_Provider_GetUser(provider,(uint32_t)user_id(),1,0,&user)==0);
  AB_ACCOUNT *account=AB_Provider_CreateAccountObject(provider);AB_Account_SetUserId(account,(uint32_t)user_id());
  AB_Account_SetCountry(account,"de");AB_Account_SetBankCode(account,"12345678");AB_Account_SetAccountNumber(account,"1234567890");
  AB_Account_SetIban(account,debtor);AB_Account_SetBic(account,"COBADEFFXXX");AB_Account_SetCurrency(account,"EUR");
  AH_DIALOG *dialog=AH_Dialog_new(user,provider);assert(dialog);GWEN_MSGENGINE *engine=AH_Dialog_GetMsgEngine(dialog); GWEN_MsgEngine_SetProtocolVersion(engine,300);
  const char *formats[]={"pain.001.001.09","pain.001.001.03","pain.001.003.03"};
  for(unsigned f=0;f<3;f++) for(int version=1;version<=2;version++) for(int convert=0;convert<=1;convert++) {
    configure(user,account,engine,version,formats[f],"",convert);
    const int commands[]={AB_Transaction_CommandSepaInstantTransfer,AB_Transaction_CommandSepaCreateDatedTransfer,AB_Transaction_CommandSepaTransfer};
    const char *codes[]={"HKIPZ","HKCSE","HKCCS"};
    for(unsigned i=0;i<3;i++) {
      AH_JOB *j=job(provider,user,account,commands[i]);assert(!strcmp(AH_Job_GetCode(j),codes[i]));
      assert(AH_Job_GetFlags(j)&AH_JOB_FLAGS_NEEDTAN);
      AB_TRANSACTION *t=transfer(commands[i],i==1?5:-1);assert(AH_Job_HandleCommand(j,t)==0);assert(AH_Job_Prepare(j)==0);
      GWEN_DB_NODE *args=AH_Job_GetArguments(j);unsigned size=0;const char *xml=GWEN_DB_GetBinValue(args,"transfer",0,NULL,0,&size);assert(xml && size);
      char *copy=malloc(size+1);memcpy(copy,xml,size);copy[size]=0;
      assert((strstr(copy,">INST<")!=NULL)==(i==0));assert(strstr(copy,"OFFLINE-ONLY-123"));
      if(i==0) assert(strstr(copy,"1999-01-01"));
      char path[1024];snprintf(path,sizeof(path),"%s/%s-%s.xml",argv[2],codes[i],formats[f]);FILE *out=fopen(path,"wb");assert(out);fwrite(copy,1,size,out);fclose(out);
      if(i<2) {assert(AH_Job_Prepare(j)==0);unsigned second=0;const void *again=GWEN_DB_GetBinValue(args,"transfer",0,NULL,0,&second);assert(second==size && !memcmp(copy,again,size));assert_wire(j,engine,codes[i],i==0?version:1);}
      free(copy);AH_Job_free(j);AB_Transaction_free(t);
    }
  }
  configure(user,account,engine,2,formats[0],"",1);results(provider,user,account,engine);boundaries(provider,user,account,engine);
  const char *omit[]={"HKIPZ","HKIPS","HIIPZS","HIIPSS","VOP","HIPINS","HIVPAS"};
  for(unsigned i=0;i<sizeof(omit)/sizeof(*omit);i++) {configure(user,account,engine,2,formats[0],omit[i],1);AH_JOB *j=NULL;assert(AH_Provider_CreateHbciJob(provider,user,account,AB_Transaction_CommandSepaInstantTransfer,&j)<0 && !j);}
  configure(user,account,engine,2,formats[0],"HKCSE",1);
  AH_JOB *unsupported=NULL;assert(AH_Provider_CreateHbciJob(provider,user,account,AB_Transaction_CommandSepaCreateDatedTransfer,&unsupported)<0 && !unsupported);
  configure(user,account,engine,2,formats[0],"",1);
  GWEN_DB_NODE *ip=AH_User_GetBpdJobForParamNameAndVersion(user,"HIIPZS",2);
  GWEN_DB_NODE *ips=AH_User_GetBpdJobForParamNameAndVersion(user,"HIIPSS",1);
  GWEN_DB_DeleteVar(ip,"supportedFormat");GWEN_DB_DeleteVar(ips,"supportedFormat");
  AH_JOB *fallback=job(provider,user,account,AB_Transaction_CommandSepaInstantTransfer);AH_Job_free(fallback); // HISPAS fallback
  GWEN_DB_SetCharValue(ip,GWEN_DB_FLAGS_OVERWRITE_VARS,"supportedFormat","urn:iso:std:iso:20022:tech:xsd:pain.001.001.99");
  assert(AH_Provider_CreateHbciJob(provider,user,account,AB_Transaction_CommandSepaInstantTransfer,&unsupported)<0 && !unsupported); // job-specific formats take precedence
  configure(user,account,engine,2,formats[0],"",1);
  ips=AH_User_GetBpdJobForParamNameAndVersion(user,"HIIPSS",1);GWEN_DB_SetIntValue(ips,GWEN_DB_FLAGS_OVERWRITE_VARS,"minWait",100);
  assert(AH_Provider_CreateHbciJob(provider,user,account,AB_Transaction_CommandSepaInstantTransfer,&unsupported)<0 && !unsupported);
  configure(user,account,engine,2,"pain.001.001.99","",1);AH_JOB *j=NULL;assert(AH_Provider_CreateHbciJob(provider,user,account,AB_Transaction_CommandSepaInstantTransfer,&j)<0 && !j);
  configure(user,account,engine,2,formats[0],"",1);
  AH_Dialog_free(dialog);assert(AB_Provider_EndExclUseUser(provider,user,0)==0);AB_User_free(user);
  AB_Account_SetOwnerName(account,"Synthetic Sender");assert(AB_Provider_AddAccount(provider,account,1)==0);
  uint32_t account_id=AB_Account_GetUniqueId(account);
  assert(AB_Provider_WriteAccountSpecForAccount(provider,account,1)==0);
  AB_Account_free(account);assert(AB_Banking_EndUseProvider(bank,provider)==0);
  adapter_dispatch(bank,account_id);
  AB_Banking_Fini(bank);AB_Banking_free(bank);bc_free(&request);GWEN_DB_Group_free(bc_config);GWEN_Gui_free(gui);GWEN_Fini();fclose(bc_output);
  puts("Offline payment protocol tests passed; no socket connections possible.");
}
