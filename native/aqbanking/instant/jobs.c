/* SPDX-License-Identifier: GPL-3.0-only
 * Included in pinned jobsepaxfersingle.c. No networking outside AqHBCI.
 * HKIPZ 1/2, HIIPZ 1/2 and HKIPS/HIIPS 1: DK CR0482 and CR0514.
 */
#include "accountjob_l.h"
#include "jobtransferbase_l.h"
#include "bankingcraft_instant_jobs.h"
#include <aqhbci/banking/user_l.h>
#include <aqhbci/applayer/hhd_l.h>
#include <aqhbci/admjobs/jobvpp.h>
#include <aqhbci/joblayer/job_swift.h>
#include <aqhbci/joblayer/job_crypt.h>
#include <aqhbci/msglayer/bpd_l.h>
#include <aqhbci/joblayer/result_l.h>
#include <aqbanking/banking_imex.h>
#include <aqbanking/backendsupport/swiftdescr.h>
#include <gwenhywfar/gwendate.h>
#include <string.h>

static int bc_ip_allowed(AB_USER *user, AB_ACCOUNT *account, const char *code) {
  if(AH_User_GetCryptMode(user)==AH_CryptMode_Pintan) {
    const AH_BPD *bpd=AH_User_GetBpd(user);
    GWEN_DB_NODE *jobs=bpd?AH_Bpd_GetBpdJobs(bpd,AH_User_GetHbciVersion(user)):NULL;
    GWEN_DB_NODE *pin=jobs?GWEN_DB_GetGroup(jobs,GWEN_PATH_FLAGS_NAMEMUSTEXIST,code):NULL;
    if(!pin || GWEN_DB_GetIntValue(pin,"needTan",0,-1)<0) return 0;
  }
  GWEN_DB_NODE *upd=AH_User_GetUpdForAccount(user,account);
  for(GWEN_DB_NODE *n=upd?GWEN_DB_GetFirstGroup(upd):NULL;n;n=GWEN_DB_GetNextGroup(n))
    if(!strcmp(GWEN_DB_GetCharValue(n,"job",0,""),code)) return 1;
  return 0;
}
static AB_SWIFT_DESCR *bc_ip_descriptor(AH_JOB *job) {
  GWEN_DB_NODE *params=AH_Job_GetParams(job);
  AB_SWIFT_DESCR_LIST *list=GWEN_DB_GetCharValue(params,"supportedFormat",0,NULL)?
    AH_Job_GetSwiftDescriptorsSupportedByJob(job,NULL,"supportedFormat","pain",1):
    AH_Job_GetSwiftDescriptorsSupportedByUser(job,"pain",1);
  AB_SWIFT_DESCR *chosen=NULL;
  /* These are the three credit-transfer profiles with an audited INST export. */
  const char *profiles[]={"pain_001_001_09","pain_001_001_03","pain_001_003_03"};
  for(unsigned i=0;i<3 && !chosen;i++)
    for(AB_SWIFT_DESCR *d=list?AB_SwiftDescr_List_First(list):NULL;d;d=AB_SwiftDescr_List_Next(d))
      if(AB_SwiftDescr_GetAlias1(d) && !strcmp(AB_SwiftDescr_GetAlias1(d),profiles[i])) {chosen=AB_SwiftDescr_dup(d);break;}
  AB_SwiftDescr_List_free(list); return chosen;
}
static int bc_ip_prepare(AH_JOB *job) {
  int scheduled=AH_Job_GetSupportedCommand(job)==AB_Transaction_CommandSepaCreateDatedTransfer;
  GWEN_DB_NODE *args=AH_Job_GetArguments(job);
  if(GWEN_DB_GetIntValue(args,"bcPrepared",0,0)) return 0; /* VoP authorizes these exact bytes. */
  AB_SWIFT_DESCR *descriptor=bc_ip_descriptor(job);
  if(!descriptor) return GWEN_ERROR_NOT_SUPPORTED;
  GWEN_DB_SetCharValue(args,GWEN_DB_FLAGS_OVERWRITE_VARS,"descriptor",AB_SwiftDescr_GetAlias2(descriptor));
  GWEN_DB_NODE *profile=AB_Banking_GetImExporterProfile(AH_Job_GetBankingApi(job),"xml",AB_SwiftDescr_GetAlias1(descriptor));
  AB_SwiftDescr_free(descriptor);
  if(!profile) return GWEN_ERROR_NOT_SUPPORTED;
  if(!scheduled) GWEN_DB_SetCharValue(profile,GWEN_DB_FLAGS_OVERWRITE_VARS,"LocalInstrumentSEPACode","INST");
  const AB_TRANSACTION *t=AH_Job_GetFirstTransfer(job);
  if(!t || AB_Transaction_List_Next(t)) {GWEN_DB_Group_free(profile);return GWEN_ERROR_INVALID;}
  AB_IMEXPORTER_CONTEXT *ctx=AB_ImExporterContext_new();
  AB_TRANSACTION *copy=AB_Transaction_dup(t);
  if(!scheduled) {
    GWEN_DATE *date=GWEN_Date_fromString("19990101"); AB_Transaction_SetDate(copy,date); GWEN_Date_free(date);
  }
  AH_Job_TransferBase_SetSumValues(job,AB_Transaction_GetValue(t));
  AB_ImExporterContext_AddTransaction(ctx,copy);
  GWEN_BUFFER *buffer=GWEN_Buffer_new(NULL,2048,0,1);
  int rv=AB_Banking_ExportToBuffer(AH_Job_GetBankingApi(job),"xml",ctx,buffer,profile);
  if(rv>=0) {
    GWEN_DB_SetBinValue(args,GWEN_DB_FLAGS_OVERWRITE_VARS,"transfer",GWEN_Buffer_GetStart(buffer),GWEN_Buffer_GetUsedBytes(buffer));
    GWEN_DB_SetIntValue(args,GWEN_DB_FLAGS_OVERWRITE_VARS,"bcPrepared",1);
  }
  GWEN_Buffer_free(buffer); AB_ImExporterContext_free(ctx); GWEN_DB_Group_free(profile);
  return rv;
}
static int bc_ip_handle(AH_JOB *job,const AB_TRANSACTION *t) {
  if(AB_Transaction_GetDate(t) || AB_Transaction_GetCommand(t)!=AB_Transaction_CommandSepaInstantTransfer ||
     AH_Job_GetFirstTransfer(job)) return GWEN_ERROR_INVALID;
  int rv=AH_Job_TransferBase_HandleCommand_SepaUndated(job,t);
  if(rv<0) return rv;
  return bc_ip_prepare(job); /* validates the format before a dialog opens */
}
static int bc_ip_status_handle(AH_JOB *job,const AB_TRANSACTION *t) {
  const char *id=AB_Transaction_GetFiId(t);
  if(!id || !*id || strlen(id)>396 || AB_Transaction_GetCommand(t)!=AB_Transaction_CommandGetSepaInstantStatus)
    return GWEN_ERROR_INVALID;
  GWEN_DB_SetCharValue(AH_Job_GetArguments(job),GWEN_DB_FLAGS_OVERWRITE_VARS,"referenceId",id);
  return 0;
}
static int bc_ip_challenge(AH_JOB *job,int version,GWEN_DB_NODE *method) {
  (void)version;(void)method;
  const AB_TRANSACTION *t=AH_Job_GetFirstTransfer(job);
  return t?AH_HHD14_AddChallengeParams_09(job,AB_Transaction_GetValue(t),AB_Transaction_GetRemoteIban(t)):GWEN_ERROR_INVALID;
}
static int bc_ip_process(AH_JOB *job,AB_IMEXPORTER_CONTEXT *ctx) {
  (void)ctx;
  GWEN_DB_NODE *args=AH_Job_GetArguments(job);
  int status_job=AH_Job_GetSupportedCommand(job)==AB_Transaction_CommandGetSepaInstantStatus;
  int scheduled=AH_Job_GetSupportedCommand(job)==AB_Transaction_CommandSepaCreateDatedTransfer;
  const char *expected=GWEN_DB_GetCharValue(args,"referenceId",0,"");
  for(GWEN_DB_NODE *n=GWEN_DB_GetFirstGroup(AH_Job_GetResponses(job));n;n=GWEN_DB_GetNextGroup(n)) {
    int rv=AH_Job_CheckEncryption(job,n); if(!rv) rv=AH_Job_CheckSignature(job,n);
    if(rv) {GWEN_DB_SetIntValue(args,GWEN_DB_FLAGS_OVERWRITE_VARS,"bcInvalid",1);AH_Job_SetStatus(job,AH_JobStatusError);return rv;}
    GWEN_DB_NODE *data=GWEN_DB_GetGroup(n,GWEN_PATH_FLAGS_NAMEMUSTEXIST,"data");
    GWEN_DB_NODE *response=data?GWEN_DB_GetGroup(data,GWEN_PATH_FLAGS_NAMEMUSTEXIST,AH_Job_GetResponseName(job)):NULL;
    if(!response) continue;
    const char *id=GWEN_DB_GetCharValue(response,"referenceId",0,"");
    const char *previous=GWEN_DB_GetCharValue(args,"bcReference",0,"");
    if((!*id && !scheduled) || strlen(id)>396 || (*previous && strcmp(previous,id)) || (status_job && strcmp(expected,id)))
      goto invalid;
    if(status_job) {
      const char *iban=GWEN_DB_GetCharValue(response,"iban",0,"");
      if(strcmp(iban,GWEN_DB_GetCharValue(args,"iban",0,""))) goto invalid;
    }
    int state=GWEN_DB_GetIntValue(response,"paymentStatus",0,0);
    if(state<0 || state>9 || (GWEN_DB_GetIntValue(args,"bcHasResponse",0,0) &&
       GWEN_DB_GetIntValue(args,"bcState",0,0)!=state)) goto invalid;
    GWEN_DB_SetCharValue(args,GWEN_DB_FLAGS_OVERWRITE_VARS,"bcReference",id);
    GWEN_DB_SetIntValue(args,GWEN_DB_FLAGS_OVERWRITE_VARS,"bcState",state);
    GWEN_DB_SetIntValue(args,GWEN_DB_FLAGS_OVERWRITE_VARS,"bcHasResponse",1);
  }
  return 0;
invalid:
  GWEN_DB_SetIntValue(args,GWEN_DB_FLAGS_OVERWRITE_VARS,"bcInvalid",1);
  AH_Job_SetStatus(job,AH_JobStatusError);
  return GWEN_ERROR_BAD_DATA;
}
static int bc_ip_results(AH_JOB *job,AB_IMEXPORTER_CONTEXT *ctx) {
  (void)ctx;
  GWEN_DB_NODE *args=AH_Job_GetArguments(job);
  int state=GWEN_DB_GetIntValue(args,"bcState",0,0);
  int malformed=GWEN_DB_GetIntValue(args,"bcInvalid",0,0);
  int has_response=GWEN_DB_GetIntValue(args,"bcHasResponse",0,0);
  int failed=0,uncertain=0,converted=0,pending=0,done=0,manual=0;
  for(int list=0;list<2;list++)
    for(AH_RESULT *r=AH_Result_List_First(list?AH_Job_GetMsgResults(job):AH_Job_GetSegResults(job));r;r=AH_Result_List_Next(r)) {
      int code=AH_Result_GetCode(r);
      if(code>=9000 || code==3001) {if(list) uncertain=1;else failed=1;}
      if(code==3270) converted=1;
      if(code==3045) pending=1;
      if(code==3046) manual=1;
      if(!list && code==20) done=1;
    }
  const char *outcome="unknown";
  int query=AH_Job_GetSupportedCommand(job)==AB_Transaction_CommandGetSepaInstantStatus;
  if(!malformed && !converted && !uncertain) {
    if(state==2 || state==5 || state==6 || state==8 || state==9) outcome="rejected";
    else if(failed) outcome=(query || state==7)?"unknown":"rejected";
    else if(has_response && (state==7 || (state==0 && done && !pending && !manual))) outcome="executed";
    else if(has_response && !manual) outcome="pending";
  }
  AB_TRANSACTION_STATUS status=!strcmp(outcome,"executed")?AB_Transaction_StatusAccepted:
    !strcmp(outcome,"rejected")?AB_Transaction_StatusRejected:AB_Transaction_StatusPending;
  AH_Job_SetStatusOnCommands(job,status);
  AB_TRANSACTION_LIST2_ITERATOR *it=AB_Transaction_List2_First(AH_Job_GetCommandList(job));
  while(it) {
    AB_TRANSACTION *t=AB_Transaction_List2Iterator_Data(it);
    if(!malformed && has_response) AB_Transaction_SetFiId(t,GWEN_DB_GetCharValue(args,"bcReference",0,NULL));
    AB_Transaction_SetMemo(t,outcome);
    AB_Transaction_SetTextKey(t,GWEN_DB_GetIntValue(args,"bcWait",0,0));
    if(!AB_Transaction_List2Iterator_Next(it)) break;
  }
  AB_Transaction_List2Iterator_free(it);
  return 0;
}
AH_JOB *BC_InstantStatus_new(AB_PROVIDER *provider,AB_USER *user,AB_ACCOUNT *account) {
  if(!bc_ip_allowed(user,account,"HKIPS")) return NULL;
  AH_JOB *job=AH_AccountJob_new("JobBCInstantStatus",provider,user,account);
  if(!job) return NULL;
  int wait=GWEN_DB_GetIntValue(AH_Job_GetParams(job),"minWait",0,-1);
  AB_SWIFT_DESCR *descriptor=bc_ip_descriptor(job);
  if(wait<0 || wait>99 || !descriptor) {AB_SwiftDescr_free(descriptor);AH_Job_free(job);return NULL;}
  GWEN_DB_NODE *args=AH_Job_GetArguments(job);
  GWEN_DB_SetCharValue(args,GWEN_DB_FLAGS_OVERWRITE_VARS,"SupportedFormats/format",AB_SwiftDescr_GetAlias2(descriptor));
  AB_SwiftDescr_free(descriptor);
  GWEN_DB_SetIntValue(args,GWEN_DB_FLAGS_OVERWRITE_VARS,"bcWait",wait);
  AH_Job_SetSupportedCommand(job,AB_Transaction_CommandGetSepaInstantStatus);
  AH_Job_SetGetLimitsFn(job,AH_Job_GetLimits_EmptyLimits);
  AH_Job_SetHandleCommandFn(job,bc_ip_status_handle);
  AH_Job_SetProcessFn(job,bc_ip_process); AH_Job_SetHandleResultsFn(job,bc_ip_results);
  return job;
}
AH_JOB *BC_InstantTransfer_new(AB_PROVIDER *provider,AB_USER *user,AB_ACCOUNT *account) {
  if(!bc_ip_allowed(user,account,"HKIPZ")) return NULL;
  AH_JOB *status=BC_InstantStatus_new(provider,user,account);
  if(!status) return NULL;
  int wait=GWEN_DB_GetIntValue(AH_Job_GetArguments(status),"bcWait",0,0); AH_Job_free(status);
  AH_JOB *vpp=AH_Job_VPP_new(provider,user,account,0);
  int verified=vpp && AH_Job_VPP_IsNeededForCode(vpp,"HKIPZ"); AH_Job_free(vpp);
  if(!verified || !AH_User_GetBpdJobForParamNameAndVersion(user,"HIVPAS",1)) return NULL; /* Never bypass payee verification with stale BPD. */
  AH_JOB *job=AH_Job_TransferBase_new("JobBCInstantTransfer",AB_Transaction_TypeTransfer,
    AB_Transaction_SubTypeEuASAP,provider,user,account);
  if(!job) return NULL;
  AB_SWIFT_DESCR *descriptor=bc_ip_descriptor(job);
  if(!descriptor) {AH_Job_free(job);return NULL;} AB_SwiftDescr_free(descriptor);
  GWEN_DB_NODE *args=AH_Job_GetArguments(job);
  GWEN_DB_SetIntValue(args,GWEN_DB_FLAGS_OVERWRITE_VARS,"bcWait",wait);
  if(AH_Job_GetSegmentVersion(job)==2) {
    const char *convert=GWEN_DB_GetCharValue(AH_Job_GetParams(job),"conversionAllowed",0,"");
    if(strcmp(convert,"J") && strcmp(convert,"N")) {AH_Job_free(job);return NULL;}
    if(!strcmp(convert,"J")) GWEN_DB_SetCharValue(args,GWEN_DB_FLAGS_OVERWRITE_VARS,"convertToSepa","N");
  }
  AH_Job_SetSupportedCommand(job,AB_Transaction_CommandSepaInstantTransfer);
  AH_Job_SetGetLimitsFn(job,AH_Job_TransferBase_GetLimits_SepaUndated);
  AH_Job_SetHandleCommandFn(job,bc_ip_handle); AH_Job_SetPrepareFn(job,bc_ip_prepare);
  AH_Job_SetChallengeClass(job,9); AH_Job_SetAddChallengeParamsFn(job,bc_ip_challenge);
  AH_Job_SetProcessFn(job,bc_ip_process); AH_Job_SetHandleResultsFn(job,bc_ip_results);
  return job;
}

/* HKCSE has its own permission, lead times and dated pain.001. No HKCCS fallback. */
static int bc_scheduled_limits(AH_JOB *job,AB_TRANSACTION_LIMITS **limits) {
  int rv=AH_Job_TransferBase_GetLimits_SepaUndated(job,limits); if(rv<0) return rv;
  GWEN_DB_NODE *p=AH_Job_GetParams(job);
  AB_TransactionLimits_SetNeedDate(*limits,1);
  AB_TransactionLimits_SetMinValueSetupTime(*limits,GWEN_DB_GetIntValue(p,"minDelay",0,0));
  AB_TransactionLimits_SetMaxValueSetupTime(*limits,GWEN_DB_GetIntValue(p,"maxDelay",0,0));
  return 0;
}
static int bc_scheduled_handle(AH_JOB *job,const AB_TRANSACTION *t) {
  if(AB_Transaction_GetCommand(t)!=AB_Transaction_CommandSepaCreateDatedTransfer ||
     AB_Transaction_GetSubType(t)==AB_Transaction_SubTypeEuASAP || !AB_Transaction_GetDate(t) ||
     AH_Job_GetFirstTransfer(job)) return GWEN_ERROR_INVALID;
  GWEN_DATE *today=GWEN_Date_CurrentDate(); int diff=GWEN_Date_Diff(AB_Transaction_GetDate(t),today); GWEN_Date_free(today);
  if(diff<=0) return GWEN_ERROR_INVALID;
  int rv=AH_Job_TransferBase_HandleCommand_SepaDated(job,t);
  return rv<0?rv:bc_ip_prepare(job);
}
static int bc_scheduled_challenge(AH_JOB *job,int version,GWEN_DB_NODE *method) {
  (void)version;(void)method;
  const AB_TRANSACTION *t=AH_Job_GetFirstTransfer(job);
  return t?AH_HHD14_AddChallengeParams_23(job,AB_Transaction_GetValue(t),AB_Transaction_GetRemoteIban(t),AB_Transaction_GetDate(t)):GWEN_ERROR_INVALID;
}
static int bc_scheduled_results(AH_JOB *job,AB_IMEXPORTER_CONTEXT *ctx) {
  (void)ctx;
  GWEN_DB_NODE *args=AH_Job_GetArguments(job); int accepted=0,failed=0;
  for(int list=0;list<2;list++)
    for(AH_RESULT *r=AH_Result_List_First(list?AH_Job_GetMsgResults(job):AH_Job_GetSegResults(job));r;r=AH_Result_List_Next(r)) {
      int code=AH_Result_GetCode(r);
      if(code>=9000 || code==3001) failed=1;
      if(!list && (code==10 || code==20)) accepted=1;
    }
  int invalid=GWEN_DB_GetIntValue(args,"bcInvalid",0,0);
  AH_Job_SetStatusOnCommands(job,invalid?AB_Transaction_StatusError:failed?AB_Transaction_StatusRejected:
    accepted?AB_Transaction_StatusAccepted:AB_Transaction_StatusPending);
  if(!invalid) {
    AB_TRANSACTION_LIST2_ITERATOR *it=AB_Transaction_List2_First(AH_Job_GetCommandList(job));
    while(it) {
      AB_Transaction_SetFiId(AB_Transaction_List2Iterator_Data(it),GWEN_DB_GetCharValue(args,"bcReference",0,NULL));
      if(!AB_Transaction_List2Iterator_Next(it)) break;
    }
    AB_Transaction_List2Iterator_free(it);
  }
  return 0;
}
AH_JOB *BC_ScheduledTransfer_new(AB_PROVIDER *provider,AB_USER *user,AB_ACCOUNT *account) {
  if(!bc_ip_allowed(user,account,"HKCSE")) return NULL;
  AH_JOB *job=AH_Job_TransferBase_new("JobBCScheduledTransfer",AB_Transaction_TypeTransfer,
    AB_Transaction_SubTypeStandard,provider,user,account);
  if(!job) return NULL;
  GWEN_DB_NODE *params=AH_Job_GetParams(job);
  int min=GWEN_DB_GetIntValue(params,"minDelay",0,-1),max=GWEN_DB_GetIntValue(params,"maxDelay",0,-1);
  AB_SWIFT_DESCR *descriptor=bc_ip_descriptor(job);
  if(min<0 || max<1 || max<min || max>9999 || !descriptor) {AB_SwiftDescr_free(descriptor);AH_Job_free(job);return NULL;}
  AB_SwiftDescr_free(descriptor);
  AH_Job_SetSupportedCommand(job,AB_Transaction_CommandSepaCreateDatedTransfer);
  AH_Job_SetGetLimitsFn(job,bc_scheduled_limits); AH_Job_SetHandleCommandFn(job,bc_scheduled_handle);
  AH_Job_SetPrepareFn(job,bc_ip_prepare); AH_Job_SetProcessFn(job,bc_ip_process);
  AH_Job_SetHandleResultsFn(job,bc_scheduled_results);
  AH_Job_SetChallengeClass(job,23); AH_Job_SetAddChallengeParamsFn(job,bc_scheduled_challenge);
  return job;
}
