/* Offline tests of the real, patched AqBanking push-approval path.
 * Only the transport is replaced. No PIN, bank connection or payment is used. */
#define PACKAGE "aqbanking"
#define AH_OutboxCBox_SendQueue bc_test_send_queue
#define AH_OutboxCBox_RecvQueue bc_test_recv_queue
#define AH_OutboxCBox_SendAndReceiveJobWithTanAndVpp bc_test_send_vop
#include <aqhbci/applayer/cbox_voptan.c>
#undef AH_OutboxCBox_SendQueue
#undef AH_OutboxCBox_RecvQueue
#undef AH_OutboxCBox_SendAndReceiveJobWithTanAndVpp

static int approval_sends, approval_result, approval_vpas, approval_complete_next;
static AH_JOB *approval_work;
int bc_test_send_queue(AH_OUTBOX_CBOX *box,AH_DIALOG *dialog,AH_JOBQUEUE *queue) {
  (void)box;(void)dialog;
  assert(!(AH_JobQueue_GetFlags(queue)&AH_JOBQUEUE_FLAGS_NEEDTAN));
  assert(!*str(AH_JobQueue_GetUsedTan(queue)));
  unsigned count=0;
  for(AH_JOB *job=AH_Job_List_First(AH_JobQueue_GetJobList(queue));job;job=AH_Job_List_Next(job)) {
    assert(job!=approval_work); // status polling must never resubmit the order
    if(!strcmp(AH_Job_GetName(job),"JobVpa")) {
      unsigned size=0;
      const void *id=GWEN_DB_GetBinValue(AH_Job_GetArguments(job),"vopId",0,NULL,0,&size);
      assert(size==13 && !memcmp(id,"SYNTHETIC-VOP",13));
      approval_vpas++; continue;
    }
    assert(!strcmp(AH_Job_GetName(job),"JobTan"));
    assert(!strcmp(GWEN_DB_GetCharValue(AH_Job_GetArguments(job),"process",0,""),"S"));
    assert(!strcmp(GWEN_DB_GetCharValue(AH_Job_GetArguments(job),"jobReference",0,""),"SYNTHETIC-ORDER-REFERENCE"));
    assert(!strcmp(GWEN_DB_GetCharValue(AH_Job_GetArguments(job),"tanMediumId",0,""),"Repaired Device"));
    assert(!(AH_Job_GetFlags(job)&AH_JOB_FLAGS_NEEDTAN));
    count++;
  }
  assert(count==1); approval_sends++;
  return 0;
}
int bc_test_recv_queue(AH_OUTBOX_CBOX *box,AH_DIALOG *dialog,AH_JOBQUEUE *queue) {
  (void)box;(void)dialog;
  if(approval_result<0) return approval_result;
  AH_JOB *job=AH_Job_List_First(AH_JobQueue_GetJobList(queue));
  while(job && strcmp(AH_Job_GetName(job),"JobTan")) job=AH_Job_List_Next(job);
  assert(job);
  GWEN_DB_NODE *response=GWEN_DB_Group_new("response");
  GWEN_DB_SetIntValue(response,GWEN_DB_FLAGS_OVERWRITE_VARS,"data/SegResult/result/resultcode",approval_result);
  AH_Job_AddResponse(job,response);
  if(approval_complete_next) approval_result=20;
  return 0;
}

static unsigned vop_dialog_calls;
static int count_vop_dialog(GWEN_GUI *gui,GWEN_DIALOG *dialog,uint32_t id) {
  (void)gui;(void)id;
  assert(!strcmp(GWEN_Dialog_GetId(dialog),"ah_vopmsg"));
  vop_dialog_calls++; return 0; // explicit rejection in the synthetic dialog
}
static void test_postbank_vop_matches(AB_BANKING *bank,AB_PROVIDER *provider,AB_USER *user) {
  AH_OUTBOX *outbox=AH_Outbox_new(provider);
  AH_OUTBOX_CBOX *box=AH_OutboxCBox_new(provider,user,outbox);
  AH_JOB *job=AH_Job_Tan_new(provider,user,4,7); assert(job);
  AH_JOB *vpp=AH_Job_VPP_new(provider,user,NULL,1); assert(vpp);
  // Populate only synthetic objects owned by the fixture job.
  AH_VOP_RESULT_LIST *results=(AH_VOP_RESULT_LIST *)AH_Job_VPP_GetResultList(vpp);
  AB_TRANSACTION *first=AB_Transaction_new();
  AB_Transaction_SetRemoteIban(first,"DE02120300000000202051"); AH_Job_AddTransfer(job,first);
  GWEN_GUI *gui=GWEN_Gui_GetGui(); GWEN_Gui_SetExecDialogFn(gui,count_vop_dialog);
  vop_dialog_calls=0;
  AB_Banking_RuntimeConfig_SetIntValue(bank,"bankingcraftSkipMatchingVop",1);
  assert(AH_OutboxCBox_LetUserConfirmVopResult(box,job,NULL,NULL)==GWEN_ERROR_USER_ABORTED);
  assert(AH_OutboxCBox_LetUserConfirmVopResult(box,job,vpp,NULL)==GWEN_ERROR_USER_ABORTED);
  assert(vop_dialog_calls==2); // absent/empty never counts as a successful match
  AH_VOP_RESULT *match=AH_VopResult_new();
  AH_VopResult_SetRemoteIban(match,"DE02120300000000202051");
  AH_VopResult_SetResult(match,AH_VopResultCodeMatch); AH_VopResult_List_Add(match,results);
  for(int with_text=0;with_text<2;with_text++) {
    assert(AH_OutboxCBox_LetUserConfirmVopResult(box,job,vpp,with_text?"Synthetic bank instruction":NULL)==0);
    assert(vop_dialog_calls==2); // screenshot case: match plus generic bank text
  }
  AB_Banking_RuntimeConfig_SetIntValue(bank,"bankingcraftSkipMatchingVop",0);
  assert(AH_OutboxCBox_LetUserConfirmVopResult(box,job,vpp,NULL)==GWEN_ERROR_USER_ABORTED);
  assert(vop_dialog_calls==3); // other bank configurations keep their dialog
  AB_Banking_RuntimeConfig_SetIntValue(bank,"bankingcraftSkipMatchingVop",1);
  const int warnings[]={AH_VopResultCodeCloseMatch,AH_VopResultCodeNoMatch,
      AH_VopResultCodeNotAvailable,AH_VopResultCodePending,AH_VopResultCodeNone};
  for(unsigned i=0;i<sizeof(warnings)/sizeof(warnings[0]);i++) {
    AH_VopResult_SetResult(match,warnings[i]);
    assert(AH_OutboxCBox_LetUserConfirmVopResult(box,job,vpp,"match")==GWEN_ERROR_USER_ABORTED);
    assert(vop_dialog_calls==4+i); // text cannot override the typed result
  }
  AH_VopResult_SetResult(match,AH_VopResultCodeMatch);
  AH_VopResult_SetRemoteIban(match,"DE89370400440532013000");
  assert(AH_OutboxCBox_LetUserConfirmVopResult(box,job,vpp,NULL)==GWEN_ERROR_USER_ABORTED);
  AH_VopResult_SetRemoteIban(match,"DE02120300000000202051");
  AB_TRANSACTION *second=AB_Transaction_new();
  AB_Transaction_SetRemoteIban(second,"DE89370400440532013000"); AH_Job_AddTransfer(job,second);
  assert(AH_OutboxCBox_LetUserConfirmVopResult(box,job,vpp,NULL)==GWEN_ERROR_USER_ABORTED);
  AH_VOP_RESULT *second_match=AH_VopResult_new();
  AH_VopResult_SetRemoteIban(second_match,"DE89370400440532013000");
  AH_VopResult_SetResult(second_match,AH_VopResultCodeMatch); AH_VopResult_List_Add(second_match,results);
  unsigned before=vop_dialog_calls;
  assert(AH_OutboxCBox_LetUserConfirmVopResult(box,job,vpp,NULL)==0 && vop_dialog_calls==before);
  AH_VopResult_SetResult(second_match,AH_VopResultCodeNoMatch);
  assert(AH_OutboxCBox_LetUserConfirmVopResult(box,job,vpp,NULL)==GWEN_ERROR_USER_ABORTED);
  assert(vop_dialog_calls==before+1); // a partial/mixed batch still prompts
  GWEN_Gui_SetExecDialogFn(gui,execute_dialog);
  AB_Banking_RuntimeConfig_SetIntValue(bank,"bankingcraftSkipMatchingVop",0);
  AH_Job_free(job); AH_Job_free(vpp); AH_OutboxCBox_free(box); AH_Outbox_free(outbox);
}

static int push_refresh_calls;
static int refresh_push_bank_info(AB_BANKING *bank,const char *command) {
  assert(!strcmp(command,"getbankinfo")); push_refresh_calls++;
  AB_PROVIDER *provider=AB_Banking_BeginUseProvider(bank,"aqhbci"); assert(provider);
  AB_USER *user=NULL; assert(AB_Provider_GetUser(provider,(uint32_t)user_id(),1,0,&user)==0);
  AH_User_AddTanMethod(user,921); AH_User_AddTanMethod(user,900);
  assert(AB_Provider_EndExclUseUser(provider,user,0)==0); AB_User_free(user);
  assert(AB_Banking_EndUseProvider(bank,provider)==0); return 0;
}
static void test_bestsign(AB_BANKING *bank) {
  AB_PROVIDER *provider=AB_Banking_BeginUseProvider(bank,"aqhbci"); assert(provider);
  AB_USER *user=NULL; assert(AB_Provider_GetUser(provider,(uint32_t)user_id(),1,0,&user)==0);
  GWEN_DB_NODE *db=GWEN_DB_Group_new("user"); assert(AB_User_WriteToDb(user,db)==0);
  GWEN_DB_NODE *bpd=GWEN_DB_GetGroup(db,GWEN_DB_FLAGS_DEFAULT,"data/backend/bpd/bpdjobs/300");
  GWEN_DB_SetIntValue(bpd,GWEN_DB_FLAGS_OVERWRITE_VARS,"HIVPPS/1/jobsPerMsg",1);
  GWEN_DB_SetIntValue(bpd,GWEN_DB_FLAGS_OVERWRITE_VARS,"HIVPAS/1/jobsPerMsg",1);
  // Reproduce Postbank's mixed list, including the legacy choice which the
  // previous build incorrectly accepted as push solely because of its name.
  GWEN_DB_DeleteGroup(bpd,"HITANS");
  const struct {int function,version; const char *name,*id; int allowed;} methods[]={
    {920,6,"BestSign","BestSign",1}, {920,7,"BestSign","BestSign",1},
    {921,7,"BestSign-Push","Decoupled",1}, {900,7,"photoTAN","photoTAN",1},
    {999,7,"BestSign-Push","Decoupled",0}};
  for(unsigned i=0;i<sizeof(methods)/sizeof(methods[0]);i++) {
    char path[32]; snprintf(path,sizeof(path),"HITANS/%d",methods[i].version);
    GWEN_DB_NODE *method=GWEN_DB_Group_new("tanMethod");
    GWEN_DB_SetIntValue(method,GWEN_DB_FLAGS_OVERWRITE_VARS,"function",methods[i].function);
    GWEN_DB_SetIntValue(method,GWEN_DB_FLAGS_OVERWRITE_VARS,"process",2);
    GWEN_DB_SetCharValue(method,GWEN_DB_FLAGS_OVERWRITE_VARS,"methodName",methods[i].name);
    GWEN_DB_SetCharValue(method,GWEN_DB_FLAGS_OVERWRITE_VARS,"methodId",methods[i].id);
    GWEN_DB_SetCharValue(method,GWEN_DB_FLAGS_OVERWRITE_VARS,"needTanMediumId","2");
    if(methods[i].function==921) {
      GWEN_DB_SetCharValue(method,GWEN_DB_FLAGS_OVERWRITE_VARS,"confirmAllowAutomatic","J");
      GWEN_DB_SetIntValue(method,GWEN_DB_FLAGS_OVERWRITE_VARS,"statusQueryMax",7);
      GWEN_DB_SetIntValue(method,GWEN_DB_FLAGS_OVERWRITE_VARS,"statusWaitFirst",2);
      GWEN_DB_SetIntValue(method,GWEN_DB_FLAGS_OVERWRITE_VARS,"statusWaitNext",3);
    }
    GWEN_DB_AddGroup(GWEN_DB_GetGroup(bpd,GWEN_DB_FLAGS_DEFAULT,path),method);
  }
  assert(AB_User_ReadFromDb(user,db)==0); GWEN_DB_Group_free(db);
  AH_User_AddTanMethod(user,900); AH_User_AddTanMethod(user,921); AH_User_SetSelectedTanMethod(user,6920);
  assert(AB_Provider_EndExclUseUser(provider,user,0)==0); AB_User_free(user);
  bc_string(&request,"preferred_tan","bestsign");
  assert(select_tan(bank,0)==0); // migrate saved 920/v6 without losing its device
  assert(select_tan(bank,0)==0); // retain BestSign and its exact device name
  assert(AB_Provider_GetUser(provider,(uint32_t)user_id(),1,1,&user)==0);
  assert(AH_User_GetSelectedTanMethod(user)==7921 && bestsign_push_selected);
  assert(!strcmp(AH_User_GetTanMediumId(user),"Repaired Device"));
  for(const AH_TAN_METHOD *method=AH_TanMethod_List_First(AH_User_GetTanMethodDescriptions(user));method;method=AH_TanMethod_List_Next(method)) {
    if(AH_TanMethod_GetFunction(method)==920) assert(is_bestsign(method) && !is_bestsign_push(method));
    if(AH_TanMethod_GetFunction(method)==921) assert(is_bestsign_push(method));
  }
  AH_TAN_METHOD *other=AH_TanMethod_new();
  AH_TanMethod_SetMethodName(other,"photoTAN"); assert(!is_bestsign(other));
  AH_TanMethod_SetMethodId(other,"BestSign"); assert(is_bestsign(other));
  // Classification depends on bank metadata, not a hard-coded 921.
  AH_TanMethod_SetFunction(other,987); AH_TanMethod_SetGvVersion(other,7);
  AH_TanMethod_SetProcess(other,2); AH_TanMethod_SetZkaTanName(other,"Decoupled");
  assert(is_bestsign_push(other));
  AH_TanMethod_SetGvVersion(other,6); assert(!is_bestsign_push(other));
  AH_TanMethod_free(other);
  AB_User_free(user);
  assert(select_tan(bank,1)==0); // visible list must exclude both legacy variants
  assert(AB_Provider_GetUser(provider,(uint32_t)user_id(),1,1,&user)==0);
  test_postbank_vop_matches(bank,provider,user);
  AH_OUTBOX *outbox=AH_Outbox_new(provider);
  AH_OUTBOX_CBOX *box=AH_OutboxCBox_new(provider,user,outbox);
  AH_DIALOG *dialog=AH_Dialog_new(user,provider);
  assert(AH_OutboxCBox_SelectItanMode(box,dialog)==0);
  assert(AH_Dialog_GetItanMethod(dialog)==921);
  assert(AH_Dialog_GetTanJobVersion(dialog)==7 && AH_Dialog_GetItanProcessType(dialog)==2);
  AH_JOB *challenge=AH_Job_Tan_new(provider,user,4,7); assert(challenge);
  GWEN_DB_NODE *response=GWEN_DB_Group_new("response");
  GWEN_DB_SetCharValue(response,GWEN_DB_FLAGS_OVERWRITE_VARS,"data/tanResponse/jobReference","SYNTHETIC-ORDER-REFERENCE");
  AH_Job_AddResponse(challenge,response);
  assert(AH_Job_Process(challenge,NULL)==0);
  approval_work=AH_Job_Tan_new(provider,user,4,7); assert(approval_work);
  // The synthetic order is unsigned; transport is a recording stub above.
  AH_Job_SubFlags(approval_work,AH_JOB_FLAGS_SIGN|AH_JOB_FLAGS_CRYPT);
  bestsign_push_selected=1; approval_sends=0;
  assert(_letUserConfirmApproval(box,approval_work,"SYNTHETIC BESTSIGN CHALLENGE")==0);
  // Missing bank text still requires an explicit approval dialog.
  assert(_letUserConfirmApproval(box,approval_work,NULL)==0);
  approval_result=3956;
  assert(_sendTanAndReceiveResponseProcS(box,dialog,challenge,NULL,approval_work)==1);
  assert(approval_sends==1 && AH_Result_GetCode(AH_Result_List_First(AH_Job_GetSegResults(approval_work)))==3956);
  approval_result=20;
  assert(_sendTanAndReceiveResponseProcS(box,dialog,challenge,NULL,approval_work)==0);
  assert(approval_sends==2 && AH_Result_List_GetCount(AH_Job_GetSegResults(approval_work))==2);
  approval_result=GWEN_ERROR_GENERIC;
  assert(_sendTanAndReceiveResponseProcS(box,dialog,challenge,NULL,approval_work)<0);
  assert(approval_sends==3); // bank rejection is propagated, never retried
  assert(_sendTanAndReceiveResponseProcS(box,dialog,challenge,NULL,approval_work)==GWEN_ERROR_USER_ABORTED);
  assert(approval_sends==3); // cancel sends nothing
  was_cancelled=0;
  AH_JOB *vpp=AH_Job_VPP_new(provider,user,NULL,1); assert(vpp);
  response=GWEN_DB_Group_new("response");
  GWEN_DB_SetBinValue(response,GWEN_DB_FLAGS_OVERWRITE_VARS,"data/vppResponse/vopId","SYNTHETIC-VOP",13);
  AH_Job_AddResponse(vpp,response); assert(AH_Job_Process(vpp,NULL)==0);
  response=GWEN_DB_Group_new("response");
  GWEN_DB_SetIntValue(response,GWEN_DB_FLAGS_OVERWRITE_VARS,"data/SegResult/result/resultcode",25);
  AH_Job_AddResponse(vpp,response);
  response=GWEN_DB_Group_new("response");
  GWEN_DB_SetIntValue(response,GWEN_DB_FLAGS_OVERWRITE_VARS,"data/SegResult/result/resultcode",3955);
  AH_Job_AddResponse(challenge,response);
  approval_result=3956; approval_complete_next=1;
  assert(_handleStage2(box,dialog,challenge,vpp,approval_work)==0);
  assert(approval_sends==5 && approval_vpas==1); // HKVPA exactly once, then only HKTAN S
  AH_Job_free(vpp);
  assert(_sendTanAndReceiveResponseProcS(box,dialog,challenge,NULL,approval_work)==GWEN_ERROR_TIMEOUT);
  assert(approval_sends==5); // bank maximum reached: no eighth acknowledgement/request
  AH_User_SetSelectedTanMethod(user,6920);
  assert(_letUserConfirmApproval(box,approval_work,"Synthetic manual fallback")==0);
  AH_User_SetSelectedTanMethod(user,7921);
  char value[16]={0}; GWEN_DB_NODE *params=GWEN_DB_Group_new("methodParams");
  GWEN_DB_SetIntValue(params,GWEN_DB_FLAGS_OVERWRITE_VARS,"tanMethodId",1);
  assert(password(NULL,GWEN_GUI_INPUT_FLAGS_TAN,NULL,"TAN","",value,1,sizeof(value),GWEN_Gui_PasswordMethod_Text,params,0)==GWEN_ERROR_NOT_SUPPORTED);
  assert(strstr(last_notice,"BestSign") && approval_sends==5 && !*value);
  GWEN_DB_Group_free(params); bestsign_push_selected=0;
  AH_Job_free(challenge); AH_Job_free(approval_work); approval_work=NULL;
  AH_Dialog_free(dialog); AH_OutboxCBox_free(box); AH_Outbox_free(outbox); AB_User_free(user);
  // A bank permission list with only legacy BestSign must fail locally,
  // rather than accepting its label as push or trying to submit a TAN.
  assert(AB_Provider_GetUser(provider,(uint32_t)user_id(),1,0,&user)==0);
  AH_User_ClearTanMethodList(user); AH_User_AddTanMethod(user,920);
  assert(AB_Provider_EndExclUseUser(provider,user,0)==0); AB_User_free(user);
  assert(select_tan(bank,0)==GWEN_ERROR_NOT_FOUND);
  assert(!bestsign_push_selected && strstr(last_notice,"BestSign-Push"));
  assert(AB_Provider_GetUser(provider,(uint32_t)user_id(),1,0,&user)==0);
  assert(AH_User_GetSelectedTanMethod(user)==7921);
  assert(!strcmp(AH_User_GetTanMediumId(user),"Repaired Device"));
  assert(AB_Provider_EndExclUseUser(provider,user,0)==0); AB_User_free(user);
  int fetched=-1; push_refresh_calls=0;
  bc_free(&request); request=event("test");
  bc_string(&request,"medium","pintan"); bc_string(&request,"preferred_tan","bestsign");
  bc_string(&request,"op","transfer");
  assert(initialize_access(bank,refresh_push_bank_info,select_tan,&fetched)==GWEN_ERROR_NOT_FOUND);
  assert(push_refresh_calls==0 && fetched==0); // no retry or refresh while sending
  bc_free(&request); request=event("test");
  bc_string(&request,"medium","pintan"); bc_string(&request,"preferred_tan","bestsign");
  bc_string(&request,"op","accounts");
  assert(initialize_access(bank,refresh_push_bank_info,select_tan,&fetched)==0);
  assert(push_refresh_calls==1 && fetched==0);
  assert(initialize_access(bank,refresh_push_bank_info,select_tan,&fetched)==0);
  assert(push_refresh_calls==1); // no unnecessary bank roundtrip after repair
  assert(AB_Banking_EndUseProvider(bank,provider)==0);
}
