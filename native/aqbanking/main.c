/* SPDX-License-Identifier: GPL-3.0-only
 * A single-access worker. AqBanking and Libchipcard own the banking and card
 * protocols. BankingCraft owns presentation, encrypted persistence and dispatch.
 */
#include "protocol.h"
#include "memory_config.h"
#include <gwenhywfar/gwenhywfar.h>
#include <gwenhywfar/gui_be.h>
#include <gwenhywfar/dialog_be.h>
#include <gwenhywfar/ssl_cert_descr.h>
#include <gwenhywfar/ctplugin.h>
#include <gwenhywfar/gwendate.h>
#include <gwenhywfar/text.h>
#include <aqbanking/banking.h>
#include <aqbanking/banking_online.h>
#include <aqbanking/banking_transaction.h>
#include <aqbanking/types/imexporter_context.h>
#include <aqbanking/types/imexporter_accountinfo.h>
#include <aqbanking/types/transaction.h>
#include <aqbanking/types/balance.h>
#include <aqbanking/banking_be.h>
#include <aqbanking/backendsupport/provider_be.h>
#include <aqbanking/backendsupport/providerqueue.h>
#include <aqhbci/banking/user.h>
#include <aqhbci/banking/user_l.h>
#include <aqhbci/banking/account.h>
#include <aqhbci/banking/provider_online.h>
#include <aqhbci/admjobs/jobgetsepainfo_l.h>
#include <chipcard/client.h>
#include <chipcard/cards/chiptanusb.h>
#include <errno.h>
#include <sys/stat.h>
#ifdef _WIN32
#include <io.h>
#include <fcntl.h>
#define dup _dup
#define fileno _fileno
#define fdopen _fdopen
#define BC_NULL "NUL"
#define strcasecmp _stricmp
#define strncasecmp _strnicmp
#else
#include <unistd.h>
#include <strings.h>
#include <sys/resource.h>
#define BC_NULL "/dev/null"
#endif

static BC_FRAME request;
// Supplied by the reviewed compatibility patch. Linking against an unpatched
// system AqBanking must fail instead of silently importing legacy cleartext data.
extern int AB_BankingCraft_NoLegacyImport(void);
extern int AB_BankingCraft_HkvvbOnlyRegistration(void);
static int was_cancelled, bad_pin, certificate_rejected;
static int unsupported_account_job;
static int card_session;
static int bestsign_push_selected, bestsign_push_missing;
static char last_notice[16384];
static unsigned notice_count;
static const char *operation_stage="initialize";
static void begin_stage(const char *stage) {
  operation_stage=stage;
  bc_wipe(last_notice,sizeof(last_notice)); notice_count=0;
}
static const char *str(const char *s) { return s?s:""; }
static BC_FRAME event(const char *name) { BC_FRAME f={0}; bc_string(&f,"event",name); return f; }
static BC_FRAME prompt(BC_FRAME *f) {
  BC_FRAME reply={0}; bc_send(f);
  if(!bc_read(&reply) || strcmp(bc_text(&reply,"op"),"answer")) exit(71);
  if(!strcmp(bc_text(&reply,"cancel"),"1")) was_cancelled=1;
  return reply;
}
static int quiet_log(GWEN_GUI *g,const char *domain,GWEN_LOGGER_LEVEL level,const char *message) {
  (void)g;(void)domain;(void)level;(void)message; return 1;
}
static int check_certificate(GWEN_GUI *g,const GWEN_SSLCERTDESCR *cert,GWEN_SYNCIO *io,uint32_t id) {
  (void)g;(void)io;(void)id;
  // Never offer a UI override for a bad/expired/untrusted TLS certificate.
  if(GWEN_SslCertDescr_GetIsError(cert)) { certificate_rejected=1; return GWEN_ERROR_SSL_SECURITY; }
  return 0;
}
static int password(GWEN_GUI *g,uint32_t flags,const char *token,const char *title,const char *text,
                    char *buffer,int min,int max,GWEN_GUI_PASSWORD_METHOD method,GWEN_DB_NODE *params,uint32_t id) {
  (void)g;(void)token;(void)method;(void)id;
  if(was_cancelled || min<0 || max<=0 || max>4096) return GWEN_ERROR_USER_ABORTED;
  // A BestSign push approval must never fall back to asking for a numeric
  // TAN. Only the native decoupled callback may return an empty approval.
  int tan_method=params?GWEN_DB_GetIntValue(params,"tanMethodId",0,1):1;
  if((flags&GWEN_GUI_INPUT_FLAGS_TAN) && bestsign_push_selected && tan_method!=AB_BANKING_TANMETHOD_DECOUPLED) {
    const char *notice="BankingCraft hat eine unerwartete TAN-Abfrage für BestSign-Push abgebrochen. Es wurde keine TAN gesendet. Bitte die Bankparameter und das ausgewählte Push-Verfahren prüfen.";
    snprintf(last_notice,sizeof(last_notice),"%s",notice); notice_count=1;
    return GWEN_ERROR_NOT_SUPPORTED;
  }
  BC_FRAME f=event("password"); bc_number(&f,"flags",flags); bc_string(&f,"title",title); bc_string(&f,"text",text);
  bc_number(&f,"min",min); bc_number(&f,"max",max);
  if(params) {
    bc_number(&f,"method",tan_method);
    if ((flags&GWEN_GUI_INPUT_FLAGS_TAN) && tan_method==AB_BANKING_TANMETHOD_DECOUPLED && GWEN_DB_GetIntValue(params,"automatic",0,0)==1) {
      bc_number(&f,"automatic",1);
      bc_number(&f,"wait_seconds",GWEN_DB_GetIntValue(params,"wait_seconds",0,-1));
      bc_number(&f,"attempt",GWEN_DB_GetIntValue(params,"attempt",0,0));
      bc_number(&f,"limit",GWEN_DB_GetIntValue(params,"limit",0,0));
    }
    bc_string(&f,"challenge",GWEN_DB_GetCharValue(params,"challenge",0,""));
    bc_string(&f,"mime",GWEN_DB_GetCharValue(params,"mimeType",0,""));
    unsigned size=0; const void *data=GWEN_DB_GetBinValue(params,"imageData",0,NULL,0,&size);
    if(size>1048576) { bc_free(&f); return GWEN_ERROR_INVALID; }
    if(data) bc_add(&f,"image",data,size);
  }
  BC_FRAME reply=prompt(&f); const char *value=bc_text(&reply,"value"); size_t n=strlen(value);
  int rv=GWEN_ERROR_USER_ABORTED;
  if(!was_cancelled && n>=(unsigned)min && n<(unsigned)max) { memcpy(buffer,value,n+1); rv=0; }
  bc_free(&reply); return rv;
}
static int input(GWEN_GUI *g,uint32_t flags,const char *title,const char *text,char *buffer,int min,int max,uint32_t id) {
  return password(g,flags,NULL,title,text,buffer,min,max,GWEN_Gui_PasswordMethod_Text,NULL,id);
}
static int password_status(GWEN_GUI *g,const char *token,const char *pin,GWEN_GUI_PASSWORD_STATUS status,uint32_t id) {
  (void)g;(void)token;(void)pin;(void)id;
  if(status==GWEN_Gui_PasswordStatus_Bad) { bad_pin=1; BC_FRAME f=event("bad_pin"); bc_send(&f); }
  return 0;
}
static int message_box(GWEN_GUI *g,uint32_t flags,const char *title,const char *text,
                       const char *b1,const char *b2,const char *b3,uint32_t id) {
  (void)g;(void)id;
  BC_FRAME f=event("message"); bc_number(&f,"flags",flags); bc_string(&f,"title",title); bc_string(&f,"text",text);
  bc_string(&f,"button1",b1); bc_string(&f,"button2",b2); bc_string(&f,"button3",b3);
  BC_FRAME reply=prompt(&f); int answer=atoi(bc_text(&reply,"value")); bc_free(&reply);
  return was_cancelled || answer<1 || answer>3?0:answer;
}
static uint32_t show_box(GWEN_GUI *g,uint32_t flags,const char *title,const char *text,uint32_t id) {
  (void)g;(void)flags;(void)id;
  static uint32_t next_box_id;
  if(was_cancelled || next_box_id==UINT32_MAX) return 0;
  uint32_t box_id=++next_box_id;
  BC_FRAME f=event("show"); bc_number(&f,"id",box_id); bc_string(&f,"text",text);
  bc_string(&f,"kind",title && !strcmp(title,"BankingCraft:reader-pin")?"reader_pin":"notice");
  bc_send(&f); return box_id;
}
static void hide_box(GWEN_GUI *g,uint32_t id) {
  (void)g; BC_FRAME f=event("hide"); bc_number(&f,"id",id); bc_send(&f);
}
static uint32_t progress_start(GWEN_GUI *g,uint32_t flags,const char *title,const char *text,uint64_t total,uint32_t id) {
  (void)g;(void)flags;(void)total;(void)id;
  // Gwenhywfar emits this for every socket wait. Keep the last meaningful
  // operation visible instead of replacing it with transport polling noise.
  if(text && !strcmp(text,"Waiting for data to become available")) return 1;
  BC_FRAME f=event("progress"); bc_string(&f,"text",text?text:title); bc_send(&f); return 1;
}
static int progress_advance(GWEN_GUI *g,uint32_t id,uint64_t progress) { (void)g;(void)id;(void)progress; return was_cancelled?GWEN_ERROR_USER_ABORTED:0; }
static int progress_end(GWEN_GUI *g,uint32_t id) { (void)g;(void)id; return 0; }
static int progress_log(GWEN_GUI *g,uint32_t id,GWEN_LOGGER_LEVEL level,const char *text) {
  (void)g;(void)id;
  // Preserve the original rejection. Dialog cleanup can report 9999 afterwards
  // and must not replace the useful error with "dialog already closed".
  if(text && *text && level<=GWEN_LoggerLevel_Warning && notice_count<8) {
    size_t used=strlen(last_notice), length=strlen(text);
    int duplicate=0;
    for(const char *p=last_notice;*p;) {
      const char *end=strchr(p,'\n'); size_t n=end?(size_t)(end-p):strlen(p);
      if(n==length && !memcmp(p,text,n)) { duplicate=1; break; }
      if(!end) break;
      p=end+1;
    }
    if(!duplicate && length+used+(used?1:0)<sizeof(last_notice)) {
      if(used) last_notice[used++]='\n';
      memcpy(last_notice+used,text,length+1); notice_count++;
    }
  }
  BC_FRAME f=event("progress"); bc_string(&f,"text",text); bc_send(&f); return 0;
}

/* VOP dialogs requested by AqBanking are displayed in full. The reviewed
 * library policy omits complete Postbank matches before creating a dialog. Other
 * dialogs are rejected until an explicit adapter exists; there is no implicit
 * acceptance of an unknown banking dialog. */
static GWEN_BUFFER *dialog_text;
static int dialog_overflow;
static int dialog_char(GWEN_DIALOG *dlg,GWEN_WIDGET *w,GWEN_DIALOG_PROPERTY prop,int index,const char *value,int signal) {
  (void)dlg;(void)w;(void)index;(void)signal;
  if(value && (prop==GWEN_DialogProperty_Title || prop==GWEN_DialogProperty_Value || prop==GWEN_DialogProperty_AddValue)) {
    if(GWEN_Buffer_GetUsedBytes(dialog_text)+strlen(value)>131072) { dialog_overflow=1; return GWEN_ERROR_INVALID; }
    GWEN_Buffer_AppendString(dialog_text,value); GWEN_Buffer_AppendString(dialog_text,"\n");
  }
  return 0;
}
static int dialog_int(GWEN_DIALOG *d,GWEN_WIDGET *w,GWEN_DIALOG_PROPERTY p,int i,int v,int s) {(void)d;(void)w;(void)p;(void)i;(void)v;(void)s;return 0;}
static int execute_dialog(GWEN_GUI *g,GWEN_DIALOG *dlg,uint32_t id) {
  (void)g;(void)id;
  if(strcmp(str(GWEN_Dialog_GetId(dlg)),"ah_vopmsg")) return GWEN_ERROR_NOT_SUPPORTED;
  dialog_text=GWEN_Buffer_new(NULL,1024,0,1);
  dialog_overflow=0;
  GWEN_Dialog_SetSetCharPropertyFn(dlg,dialog_char); GWEN_Dialog_SetSetIntPropertyFn(dlg,dialog_int);
  int rv=GWEN_Dialog_EmitSignal(dlg,GWEN_DialogEvent_TypeInit,NULL);
  if(dialog_overflow) rv=GWEN_ERROR_INVALID;
  if(rv>=0) {
    BC_FRAME f=event("vop"); bc_string(&f,"text",GWEN_Buffer_GetStart(dialog_text));
    BC_FRAME reply=prompt(&f); int accepted=!was_cancelled && !strcmp(bc_text(&reply,"value"),"1");
    bc_free(&reply);
    int action=GWEN_Dialog_EmitSignal(dlg,GWEN_DialogEvent_TypeActivated,accepted?"acceptButton":"abortButton");
    // Dialog event enum values are not ExecDialog return codes: Reject is
    // positive too. Only an explicit Accept event maps to successful approval.
    rv=accepted && action==GWEN_DialogEvent_ResultAccept?1:0;
  }
  GWEN_Dialog_EmitSignal(dlg,GWEN_DialogEvent_TypeFini,NULL);
  GWEN_Buffer_free(dialog_text); dialog_text=NULL; return rv;
}
static GWEN_GUI *setup_gui(void) {
  GWEN_GUI *g=GWEN_Gui_new();
  GWEN_Gui_SetGetPasswordFn(g,password); GWEN_Gui_SetInputBoxFn(g,input);
  GWEN_Gui_SetSetPasswordStatusFn(g,password_status); GWEN_Gui_SetMessageBoxFn(g,message_box);
  GWEN_Gui_SetShowBoxFn(g,show_box); GWEN_Gui_SetHideBoxFn(g,hide_box);
  GWEN_Gui_SetProgressStartFn(g,progress_start); GWEN_Gui_SetProgressAdvanceFn(g,progress_advance);
  GWEN_Gui_SetProgressEndFn(g,progress_end); GWEN_Gui_SetProgressLogFn(g,progress_log);
  GWEN_Gui_SetLogHookFn(g,quiet_log); GWEN_Gui_SetCheckCertFn(g,check_certificate); GWEN_Gui_SetExecDialogFn(g,execute_dialog);
  GWEN_Gui_SetFlags(g,GWEN_Gui_GetFlags(g)&~GWEN_GUI_FLAGS_PERMPASSWORDS); GWEN_Gui_SetGui(g); return g;
}
static void emit_db(BC_FRAME *f,const char *key,GWEN_DB_NODE *db) {
  GWEN_BUFFER *b=GWEN_Buffer_new(NULL,1024,0,1);
  if(GWEN_DB_WriteToBuffer(db,b,GWEN_DB_FLAGS_DEFAULT)<0) exit(71);
  bc_add(f,key,GWEN_Buffer_GetStart(b),GWEN_Buffer_GetUsedBytes(b));
  bc_wipe(GWEN_Buffer_GetStart(b),GWEN_Buffer_GetUsedBytes(b)); GWEN_Buffer_free(b);
}
static void emit_date(BC_FRAME *f,const char *key,const GWEN_DATE *d) { bc_string(f,key,d?GWEN_Date_GetString(d):""); }
static int emit_value(BC_FRAME *f,const AB_VALUE *v) {
  char amount[1024];
  if(!v || AB_Value_GetNumDenomString(v,amount,sizeof(amount))<0) {
    progress_log(NULL,0,GWEN_LoggerLevel_Error,
        "AqBanking hat einen Datensatz ohne gültigen Betrag geliefert. Der Abruf für dieses Konto wurde nicht vollständig übernommen.");
    return GWEN_ERROR_BAD_DATA;
  }
  bc_string(f,"amount",amount); bc_string(f,"currency",AB_Value_GetCurrency(v));
  return 0;
}
static int user_id(void) {
  GWEN_DB_NODE *specs=GWEN_DB_GetGroup(bc_config,GWEN_PATH_FLAGS_NAMEMUSTEXIST,"groups/users");
  if(!specs) return 0;
  GWEN_DB_NODE *user=GWEN_DB_GetFirstGroup(specs);
  if(!user || GWEN_DB_GetNextGroup(user)) return 0; // one access per isolated store
  return GWEN_DB_GetIntValue(user,"uniqueId",0,0);
}
static int control(AB_BANKING *ab,const char *cmd) {
  begin_stage(cmd);
  if(card_session && (!strcmp(cmd,"getkeys") || !strcmp(cmd,"getsysid") || !strcmp(cmd,"getaccounts"))) {
    // The command-line wrappers unmount the card after every setup step.
    // AqHBCI's documented nounmount argument retains it within this read session.
    AB_PROVIDER *provider=AB_Banking_BeginUseProvider(ab,"aqhbci");
    if(!provider) return GWEN_ERROR_NOT_FOUND;
    AB_USER *user=NULL;
    int rv=AB_Provider_GetUser(provider,(uint32_t)user_id(),1,1,&user);
    if(rv==0) {
      AB_IMEXPORTER_CONTEXT *ctx=AB_ImExporterContext_new();
      if(!strcmp(cmd,"getkeys")) rv=AH_Provider_GetServerKeys(provider,user,ctx,1,1,1);
      else if(!strcmp(cmd,"getsysid")) rv=AH_Provider_GetSysId(provider,user,ctx,1,1,1);
      else rv=AH_Provider_GetAccounts(provider,user,ctx,1,1,1);
      AB_ImExporterContext_free(ctx); AB_User_free(user);
    }
    int ended=AB_Banking_EndUseProvider(ab,provider);
    return rv==0?ended:rv;
  }
  char id[32]; snprintf(id,sizeof(id),"%d",user_id());
  char *argv[]={(char *)cmd,"-u",id};
  return AB_Banking_ProviderControl(ab,"aqhbci",3,argv);
}
static int resolve_auto_card_context(uint32_t *selected,GWEN_BUFFER *token_name);
static int ensure_user_with(AB_BANKING *ab,
    int (*resolve)(uint32_t *,GWEN_BUFFER *),
    int (*dispatch)(AB_BANKING *,const char *,int,char **)) {
  if(user_id()) return 0;
  const char *medium=bc_text(&request,"medium");
  uint32_t selected=(uint32_t)strtoul(bc_text(&request,"record"),NULL,10);
  GWEN_BUFFER *token_name=GWEN_Buffer_new(NULL,64,0,1);
  // Imported accesses use 0 for automatic matching, never a literal card ID.
  if(!selected && (!strcmp(medium,"zkacard") || !strcmp(medium,"ddvcard"))) {
    begin_stage("select_card_context");
    int rv=resolve(&selected,token_name);
    if(rv!=0 || !selected || !GWEN_Buffer_GetUsedBytes(token_name)) {
      GWEN_Buffer_free(token_name);
      return rv!=0?rv:GWEN_ERROR_BAD_DATA;
    }
  }
  begin_stage("initialize");
  char context[32]; snprintf(context,sizeof(context),"--context=%u",selected);
  char *argv[]={"adduser","-N","BankingCraft","-b",(char *)bc_text(&request,"blz"),
    "-u",(char *)bc_text(&request,"user"),"-c",(char *)bc_text(&request,"customer"),
    "-t",(char *)bc_text(&request,"medium"),"-s",(char *)bc_text(&request,"url"),
    "--hbciversion=300",context,"--rdhtype=1","-n",GWEN_Buffer_GetStart(token_name)};
  // The initial profile 1 lets AqBanking adopt the actual profile from the card.
  // Bind the second open to the same card whose context was just resolved.
  int argc=(int)(sizeof(argv)/sizeof(argv[0]));
  if(!GWEN_Buffer_GetUsedBytes(token_name)) argc-=2;
  int rv=dispatch(ab,"aqhbci",argc,argv);
  GWEN_Buffer_free(token_name);
  if(rv!=0 && !*last_notice && !was_cancelled)
    progress_log(NULL,0,GWEN_LoggerLevel_Error,
        "AqBanking konnte den Bankzugang nicht anlegen. Bei Chipkartenzugängen bitte Karte, Kartenleser und Bankeintrag prüfen.");
  return rv;
}
static int ensure_user(AB_BANKING *ab) {
  return ensure_user_with(ab,resolve_auto_card_context,AB_Banking_ProviderControl);
}
static int upgrade_snapshot(AB_BANKING *ab) {
  int format=GWEN_DB_GetIntValue(bc_config,"snapshot_format",0,0);
  if(format==2) return 0;
  if(format>2) return GWEN_ERROR_BAD_DATA;
  // Older snapshots may already contain merged UPD/TAN groups. Their original
  // associations cannot be reconstructed safely. Keep account IDs/history and
  // the chosen medium, but request fresh BPD/UPD during the next normal setup.
  begin_stage("initialize");
  AB_PROVIDER *provider=AB_Banking_BeginUseProvider(ab,"aqhbci");
  if(!provider) return GWEN_ERROR_NOT_FOUND;
  AB_USER *user=NULL;
  int rv=AB_Provider_GetUser(provider,(uint32_t)user_id(),1,0,&user);
  if(rv==0) {
    AH_User_SetBpdVersion(user,0); AH_User_SetUpdVersion(user,0);
    rv=AB_Provider_EndExclUseUser(provider,user,0); AB_User_free(user);
  }
  int ended=AB_Banking_EndUseProvider(ab,provider); if(rv==0) rv=ended;
  if(rv==0) {
    GWEN_DB_DeleteVar(bc_config,"initialized");
    GWEN_DB_DeleteVar(bc_config,"sepa_initialized");
    GWEN_DB_SetIntValue(bc_config,GWEN_DB_FLAGS_OVERWRITE_VARS,"snapshot_format",2);
  }
  return rv;
}
static int valid_medium_name(const char *name) {
  int length=GWEN_Text_CountUtf8Chars(name,0), nonspace=0;
  if(length<1 || length>32) return 0;
  for(const unsigned char *p=(const unsigned char *)name;*p;p++) {
    if(*p<32 || *p==127) return 0;
    if(*p!=' ') nonspace=1;
  }
  return nonspace;
}
static int configure_tan_medium(AB_USER *user,const AH_TAN_METHOD *method,int force) {
  if(!AH_TanMethod_GetNeedTanMediumId(method)) return 0;
  if(!force && valid_medium_name(str(AH_User_GetTanMediumId(user)))) return 0;
  begin_stage("select_tan_medium");
  const char *message="Bitte den exakten Namen des bei der Bank eingerichteten TAN-Mediums eingeben. "
      "Bei BestSign ist dies die selbst vergebene Bezeichnung des BestSign-Geräts in den Sicherheitseinstellungen des Online-Bankings. "
      "Groß-/Kleinschreibung und Leerzeichen beibehalten (1 bis 32 Zeichen).";
  for(;;) {
    char name[129]={0}; // up to 32 UTF-8 characters, not just 32 bytes
    int rv=input(NULL,GWEN_GUI_INPUT_FLAGS_SHOW,"Name des TAN-Mediums",message,name,1,sizeof(name),0);
    if(rv<0) { bc_wipe(name,sizeof(name)); return rv; }
    if(valid_medium_name(name)) {
      AH_User_SetTanMediumId(user,name); bc_wipe(name,sizeof(name)); return 0;
    }
    bc_wipe(name,sizeof(name));
    message="Bitte einen Namen mit 1 bis 32 Zeichen ohne Zeilenumbrüche eingeben. "
        "Verwende genau die Bezeichnung des TAN-Mediums aus dem Online-Banking deiner Bank.";
  }
}
static int is_bestsign(const AH_TAN_METHOD *method) {
  const char *values[]={AH_TanMethod_GetMethodName(method),AH_TanMethod_GetMethodId(method)};
  for(unsigned i=0;i<sizeof(values)/sizeof(values[0]);i++)
    for(const char *p=str(values[i]);*p;p++)
      if(!strncasecmp(p,"bestsign",8)) return 1;
  return 0;
}
static int is_bestsign_push(const AH_TAN_METHOD *method) {
  // BestSign (e.g. function 920/v6) is a TAN-entry method. Only the bank's
  // separate decoupled method may be used for a requested push approval.
  // A display name containing "BestSign" or version 7 alone is insufficient.
  if(!is_bestsign(method) || AH_TanMethod_GetGvVersion(method)<7 || AH_TanMethod_GetProcess(method)!=2) return 0;
  const char *id=str(AH_TanMethod_GetMethodId(method));
  const char *zka=str(AH_TanMethod_GetZkaTanName(method));
  const char *name=str(AH_TanMethod_GetMethodName(method));
  return !strcasecmp(zka,"Decoupled") || !strcasecmp(id,"Decoupled") ||
      !strncasecmp(id,"Decoupled.",10) || !strcasecmp(name,"BestSign-Push");
}
static int select_tan(AB_BANKING *ab,int force) {
  begin_stage("select_tan");
  bestsign_push_selected=0; bestsign_push_missing=0;
  if(strcmp(bc_text(&request,"medium"),"pintan")) return 0;
  int prefer_bestsign=!strcmp(bc_text(&request,"preferred_tan"),"bestsign");
  AB_PROVIDER *provider=AB_Banking_BeginUseProvider(ab,"aqhbci");
  if(!provider) return GWEN_ERROR_NOT_FOUND;
  AB_USER *user=NULL; int rv=AB_Provider_GetUser(provider,(uint32_t)user_id(),1,0,&user);
  if(rv==0) {
    const AH_TAN_METHOD_LIST *methods=AH_User_GetTanMethodDescriptions(user);
    int current=AH_User_GetSelectedTanMethod(user), values[64], count=0, valid=0;
    const AH_TAN_METHOD *previous=NULL;
    for(const AH_TAN_METHOD *m=methods?AH_TanMethod_List_First(methods):NULL;m;m=AH_TanMethod_List_Next(m))
      if(AH_TanMethod_GetFunction(m)+1000*AH_TanMethod_GetGvVersion(m)==current) { previous=m; break; }
    BC_FRAME f=event("choice"); bc_string(&f,"title","TAN-Verfahren");
    bc_string(&f,"text",prefer_bestsign?
        "Bitte BestSign-Push für die Freigabe in der Postbank-App oder am BestSign-Gerät wählen. Das ältere BestSign-Verfahren mit TAN-Eingabe wird hier nicht verwendet.":
        "Bitte das bei der Bank eingerichtete TAN-Verfahren wählen.");
    for(const AH_TAN_METHOD *m=methods?AH_TanMethod_List_First(methods):NULL;m;m=AH_TanMethod_List_Next(m)) {
      int function=AH_TanMethod_GetFunction(m);
      if(!AH_User_HasTanMethod(user,function) || (prefer_bestsign && !is_bestsign_push(m))) continue;
      int combined=function+1000*AH_TanMethod_GetGvVersion(m);
      if(combined==current) valid=1;
      if(count>=60) { rv=GWEN_ERROR_INVALID; break; }
      char key[24], label[512]; snprintf(key,sizeof(key),"%d",combined);
      snprintf(label,sizeof(label),"%s (%d, v%d)",str(AH_TanMethod_GetMethodName(m)),function,AH_TanMethod_GetGvVersion(m));
      if(bc_field(&f,key)) continue;
      bc_string(&f,key,label); values[count++]=combined;
    }
    if(rv==0 && (force || !valid)) {
      if(!count) {
        bestsign_push_missing=prefer_bestsign;
        if(prefer_bestsign) progress_log(NULL,0,GWEN_LoggerLevel_Error,
            "Die gespeicherten Bankparameter enthalten kein für diesen Zugang zugelassenes BestSign-Push-Verfahren. BestSign mit TAN-Eingabe genügt für die App-Freigabe nicht. Bitte den Bankzugang synchronisieren.");
        rv=GWEN_ERROR_NOT_FOUND;
      }
      else {
        BC_FRAME answer={0};
        // An existing legacy BestSign selection can migrate locally to the
        // sole advertised push method; no bank order is sent by this step.
        int migrate=!force && prefer_bestsign && count==1 && previous && is_bestsign(previous);
        if(!migrate) answer=prompt(&f);
        int chosen=migrate?values[0]:atoi(bc_text(&answer,"value")), found=0;
        for(int i=0;i<count;i++) if(chosen==values[i]) found=1;
        if(was_cancelled || !found) rv=GWEN_ERROR_USER_ABORTED;
        else {
          AH_User_SetSelectedTanMethod(user,chosen);
          // AqBanking presents HHD/QR through our UI. Its legacy USB input
          // parser is bypassed; the bounded Libchipcard adapter serves USB.
          AH_User_SetSelectedTanInputMechanism(user,0);
          // Legacy BestSign and BestSign-Push use the same named device.
          // Keep its exact spelling for this upgrade; other method changes
          // still require selecting the appropriate medium again.
          if(chosen!=current && !(prefer_bestsign && previous && is_bestsign(previous)))
            AH_User_SetTanMediumId(user,NULL);
        }
        bc_free(&answer);
      }
    }
    bc_free(&f);
    if(rv==0) {
      const AH_TAN_METHOD *selected=NULL;
      for(const AH_TAN_METHOD *m=methods?AH_TanMethod_List_First(methods):NULL;m;m=AH_TanMethod_List_Next(m)) {
        if(AH_TanMethod_GetFunction(m)+1000*AH_TanMethod_GetGvVersion(m)==AH_User_GetSelectedTanMethod(user)) { selected=m; break; }
      }
      rv=selected?configure_tan_medium(user,selected,force):GWEN_ERROR_NOT_FOUND;
      if(rv==0) bestsign_push_selected=is_bestsign_push(selected);
    }
    int saved=AB_Provider_EndExclUseUser(provider,user,rv!=0);
    AB_User_free(user); if(rv==0) rv=saved;
  }
  int ended=AB_Banking_EndUseProvider(ab,provider); if(rv==0) rv=ended;
  return rv;
}
static int get_sepa_accounts(AB_BANKING *ab) {
  begin_stage("getaccsepa");
  AB_PROVIDER *provider=AB_Banking_BeginUseProvider(ab,"aqhbci");
  if(!provider) return GWEN_ERROR_NOT_FOUND;
  AB_USER *user=NULL;
  int rv=AB_Provider_GetUser(provider,(uint32_t)user_id(),1,1,&user);
  if(rv==0) {
    // Ask AqBanking whether the bank offers HKSPA; unsupported banks need no
    // extra dialog. A supported but rejected request remains an access error.
    AH_JOB *job=AH_Job_GetAccountSepaInfo_new(provider,user);
    if(job) {
      AH_Job_free(job);
      AB_IMEXPORTER_CONTEXT *ctx=AB_ImExporterContext_new();
      rv=AH_Provider_GetAccountSepaInfo(provider,user,ctx,1,card_session,1);
      AB_ImExporterContext_free(ctx);
    }
    AB_User_free(user);
  }
  int ended=AB_Banking_EndUseProvider(ab,provider); if(rv==0) rv=ended;
  return rv;
}
static int ensure_sepa_accounts(AB_BANKING *ab,int refreshed,int (*fetch)(AB_BANKING *)) {
  // This also upgrades existing initialized snapshots. Fetch once per account
  // discovery, not once per balance/statement job. Never mark a failed fetch as
  // complete or replace identifiers with values guessed from a local IBAN.
  if(refreshed) GWEN_DB_DeleteVar(bc_config,"sepa_initialized");
  if(GWEN_DB_GetIntValue(bc_config,"sepa_initialized",0,0)) return 0;
  int rv=fetch(ab);
  if(rv==0) GWEN_DB_SetIntValue(bc_config,GWEN_DB_FLAGS_OVERWRITE_VARS,"sepa_initialized",1);
  return rv;
}
static int account_allows_camt(const AB_USER *user,const AB_ACCOUNT *account) {
  GWEN_DB_NODE *upd=AH_User_GetUpdForAccount(user,account);
  for(GWEN_DB_NODE *job=upd?GWEN_DB_GetFirstGroup(upd):NULL;job;job=GWEN_DB_GetNextGroup(job))
    if(!strcasecmp(GWEN_DB_GetCharValue(job,"job",0,""),"HKCAZ")) return 1;
  return 0;
}
static int prefer_camt_accounts(AB_BANKING *ab) {
  begin_stage("configure_accounts");
  AB_ACCOUNT_SPEC_LIST *specs=NULL; int rv=AB_Banking_GetAccountSpecList(ab,&specs);
  if(rv<0) return rv;
  AB_PROVIDER *provider=AB_Banking_BeginUseProvider(ab,"aqhbci");
  if(!provider) { AB_AccountSpec_List_free(specs); return GWEN_ERROR_NOT_FOUND; }
  for(AB_ACCOUNT_SPEC *spec=AB_AccountSpec_List_First(specs);spec;spec=AB_AccountSpec_List_Next(spec)) {
    AB_ACCOUNT *account=NULL;
    rv=AB_Provider_GetAccount(provider,AB_AccountSpec_GetUniqueId(spec),1,0,&account);
    if(rv<0) break;
    AB_USER *user=NULL;
    rv=AB_Provider_GetUser(provider,AB_Account_GetUserId(account),1,1,&user);
    if(rv==0) {
      // 6.9.2's CAMT factory checks bank BPD but doesn't require an account's
      // HKCAZ permission. Use AqBanking's parsed UPD for this exact account,
      // including its native UID/subaccount mapping. Clear our old blanket
      // preference so card/SWIFT jobs and unsupported accounts stay distinct.
      if(account_allows_camt(user,account)) AH_Account_AddFlags(account,AH_BANK_FLAGS_PREFER_CAMT_DOWNLOAD);
      else AH_Account_SubFlags(account,AH_BANK_FLAGS_PREFER_CAMT_DOWNLOAD);
      AB_User_free(user);
    }
    int saved=AB_Provider_EndExclUseAccount(provider,account,rv!=0); if(rv==0) rv=saved;
    // Rebuild limits after SEPA/UPD changes, before exposing capabilities.
    if(rv==0) rv=AB_Provider_WriteAccountSpecForAccount(provider,account,1);
    AB_Account_free(account);
    if(rv<0) break;
  }
  int ended=AB_Banking_EndUseProvider(ab,provider); if(rv==0) rv=ended;
  AB_AccountSpec_List_free(specs); return rv;
}
static int emit_accounts(AB_BANKING *ab) {
  AB_ACCOUNT_SPEC_LIST *list=NULL; int rv=AB_Banking_GetAccountSpecList(ab,&list);
  if(rv<0) return rv;
  for(AB_ACCOUNT_SPEC *a=AB_AccountSpec_List_First(list);a;a=AB_AccountSpec_List_Next(a)) {
    BC_FRAME f=event("account"); bc_number(&f,"id",AB_AccountSpec_GetUniqueId(a)); bc_number(&f,"kind",AB_AccountSpec_GetType(a));
#define ACCOUNT(key,field) bc_string(&f,key,AB_AccountSpec_Get##field(a))
    ACCOUNT("iban",Iban); ACCOUNT("bic",Bic); ACCOUNT("number",AccountNumber); ACCOUNT("sub",SubAccountNumber);
    ACCOUNT("blz",BankCode); ACCOUNT("owner",OwnerName); ACCOUNT("name",AccountName); ACCOUNT("currency",Currency);
#undef ACCOUNT
    GWEN_BUFFER *caps=GWEN_Buffer_new(NULL,128,0,1);
    const struct { int cmd; const char *code; } allowed[]={
      {AB_Transaction_CommandGetBalance,"HKSAL"},{AB_Transaction_CommandGetTransactions,"HKKAZ"},
      {AB_Transaction_CommandSepaGetStandingOrders,"HKCDB"},{AB_Transaction_CommandSepaTransfer,"HKCCS"},
      {AB_Transaction_CommandSepaCreateDatedTransfer,"HKCSE"},{AB_Transaction_CommandSepaCreateStandingOrder,"HKCDE"},
      {AB_Transaction_CommandSepaDeleteStandingOrder,"HKCDL"}};
    for(unsigned i=0;i<sizeof(allowed)/sizeof(allowed[0]);i++) if(AB_AccountSpec_GetTransactionLimitsForCommand(a,allowed[i].cmd)) {
      GWEN_Buffer_AppendString(caps,allowed[i].code); GWEN_Buffer_AppendString(caps,"\n");
    }
    bc_string(&f,"allowed",GWEN_Buffer_GetStart(caps)); GWEN_Buffer_free(caps); bc_send(&f);
  }
  AB_AccountSpec_List_free(list); return 0;
}
static int emit_transaction(const AB_TRANSACTION *t) {
  BC_FRAME f=event("transaction");
  bc_number(&f,"type",AB_Transaction_GetType(t)); bc_number(&f,"status",AB_Transaction_GetStatus(t));
  bc_number(&f,"subtype",AB_Transaction_GetSubType(t));
  emit_date(&f,"date",AB_Transaction_GetDate(t)); emit_date(&f,"value_date",AB_Transaction_GetValutaDate(t));
  int rv=emit_value(&f,AB_Transaction_GetValue(t));
  if(rv<0) { bc_free(&f); return rv; }
#define TRANSACTION(key,field) bc_string(&f,key,AB_Transaction_Get##field(t))
  TRANSACTION("name",RemoteName); TRANSACTION("iban",RemoteIban); TRANSACTION("bic",RemoteBic);
  TRANSACTION("purpose",Purpose); TRANSACTION("posting",TransactionText); TRANSACTION("end_to_end",EndToEndReference);
  TRANSACTION("mandate",MandateId); TRANSACTION("creditor",CreditorSchemeId); TRANSACTION("order_id",FiId);
#undef TRANSACTION
  bc_number(&f,"period",AB_Transaction_GetPeriod(t)); bc_number(&f,"cycle",AB_Transaction_GetCycle(t)); bc_number(&f,"day",AB_Transaction_GetExecutionDay(t));
  emit_date(&f,"first",AB_Transaction_GetFirstDate(t)); emit_date(&f,"last",AB_Transaction_GetLastDate(t));
  GWEN_DB_NODE *db=GWEN_DB_Group_new("transaction"); AB_Transaction_toDb(t,db); emit_db(&f,"native",db); GWEN_DB_Group_free(db);
  bc_send(&f);
  return 0;
}
static int emit_account_results(const AB_IMEXPORTER_ACCOUNTINFO *account) {
  for(AB_BALANCE *b=AB_ImExporterAccountInfo_GetFirstBalance(account);b;b=AB_Balance_List_Next(b)) {
    // Match the parent's parse_balance contract. CAMT may include additional
    // balance records with unknown type/no value; they are not booked or
    // disposable balances and must not prevent importing valid transactions.
    int type=AB_Balance_GetType(b);
    if(type!=AB_Balance_TypeBooked && type!=AB_Balance_TypeDisposable) continue;
    BC_FRAME f=event("balance"); bc_number(&f,"type",type); emit_date(&f,"date",AB_Balance_GetDate(b));
    int rv=emit_value(&f,AB_Balance_GetValue(b));
    if(rv<0) { bc_free(&f); return rv; }
    bc_send(&f);
  }
  for(AB_TRANSACTION *tx=AB_Transaction_List_First(AB_ImExporterAccountInfo_GetTransactionList(account));tx;tx=AB_Transaction_List_Next(tx)) {
    int rv=emit_transaction(tx); if(rv<0) return rv;
  }
  return 0;
}
/* Match AqBanking's PIN/TAN setup wizard: fetch anonymous BPD/SCA parameters
 * before system-id synchronization, then select a TAN method and fetch UPD.
 * The anonymous step is optional for banks that don't offer it, just as in
 * AqBanking's wizard; cancellation and certificate rejection remain fatal.
 * Callbacks let tests exercise failure/resume paths without any bank I/O. */
static int initialize_access(AB_BANKING *ab,
                             int (*command)(AB_BANKING *,const char *),
                             int (*choose_tan)(AB_BANKING *,int),
                             int *accounts_fetched) {
  *accounts_fetched=0;
  // Also repair older initialized snapshots that lack a required medium name.
  // This reads local AqBanking data; it does not open another bank dialog.
  if(GWEN_DB_GetIntValue(bc_config,"initialized",0,0)) {
    int rv=choose_tan(ab,0);
    const char *op=bc_text(&request,"op");
    // A requested read-only synchronization must be able to repair stale BPD
    // even when local validation can no longer accept the old TAN method.
    // Refresh once, before any job, never retry a payment or authentication.
    if(rv==GWEN_ERROR_NOT_FOUND && bestsign_push_missing &&
       (!strcmp(op,"accounts") || !strcmp(op,"open"))) {
      rv=command(ab,"getbankinfo");
      if(was_cancelled || rv==GWEN_ERROR_USER_ABORTED) return GWEN_ERROR_USER_ABORTED;
      if(certificate_rejected) return GWEN_ERROR_SSL_SECURITY;
      if(rv==0) rv=choose_tan(ab,0);
    }
    return rv;
  }
  const char *medium=bc_text(&request,"medium");
  int rv=0;
  if(!strcmp(medium,"zkacard")) rv=command(ab,"getkeys");
  if(rv==0 && !strcmp(medium,"pintan")) {
    rv=command(ab,"getbankinfo");
    if(was_cancelled || rv==GWEN_ERROR_USER_ABORTED) return GWEN_ERROR_USER_ABORTED;
    if(certificate_rejected) return GWEN_ERROR_SSL_SECURITY;
    // Some banks reject anonymous BPD requests; the authenticated sync is still
    // required and must succeed. No authenticated failure is retried here.
    rv=0;
  }
  if(rv==0 && strcmp(medium,"ddvcard")) rv=command(ab,"getsysid");
  if(rv==0) rv=choose_tan(ab,0);
  if(rv==0) rv=command(ab,"getaccounts");
  if(rv==0) {
    GWEN_DB_SetIntValue(bc_config,GWEN_DB_FLAGS_OVERWRITE_VARS,"initialized",1);
    *accounts_fetched=1;
  }
  return rv;
}

#include "read_batch.h"

static int send_card_read_with(AB_BANKING *ab,const AB_ACCOUNT_SPEC *account,AB_TRANSACTION *t,
                              AB_IMEXPORTER_CONTEXT *ctx,
                              int (*send)(AB_PROVIDER *,AB_PROVIDERQUEUE *,AB_IMEXPORTER_CONTEXT *)) {
  int cmd=AB_Transaction_GetCommand(t);
  if(cmd!=AB_Transaction_CommandGetBalance && cmd!=AB_Transaction_CommandGetTransactions)
    return GWEN_ERROR_INVALID;
  if(strcmp(str(AB_AccountSpec_GetBackendName(account)),"aqhbci") ||
     AB_Transaction_GetUniqueAccountId(t)!=AB_AccountSpec_GetUniqueId(account)) return GWEN_ERROR_INVALID;
  // Use AqBanking's provider queue API for this single-account read. Unlike
  // AB_Banking_SendCommands, it leaves crypt tokens mounted until session end.
  // AqHBCI still builds jobs, checks permissions, signs and verifies responses.
  AB_PROVIDER *provider=AB_Banking_BeginUseProvider(ab,"aqhbci");
  if(!provider) return GWEN_ERROR_NOT_FOUND;
  AB_ACCOUNTQUEUE *aq=AB_AccountQueue_new();
  AB_AccountQueue_SetAccountId(aq,AB_AccountSpec_GetUniqueId(account));
  AB_AccountQueue_SetAccountSpec(aq,AB_AccountSpec_dup(account));
  AB_Transaction_SetRefUniqueId(t,0);
  AB_Transaction_SetStatus(t,AB_Transaction_StatusEnqueued);
  AB_AccountQueue_AddTransaction(aq,t);
  AB_PROVIDERQUEUE *pq=AB_ProviderQueue_new();
  AB_ProviderQueue_SetProviderName(pq,"aqhbci");
  AB_ProviderQueue_AddAccountQueue(pq,aq);
  int rv=send(provider,pq,ctx);
  AB_ProviderQueue_free(pq);
  int ended=AB_Banking_EndUseProvider(ab,provider);
  return rv==0?ended:rv;
}

static int run_job(AB_BANKING *ab,int cmd) {
  AB_ACCOUNT_SPEC *account=NULL;
  int rv=AB_Banking_GetAccountSpecByUniqueId(ab,(uint32_t)strtoul(bc_text(&request,"account"),NULL,10),&account);
  if(rv<0) return rv;
  const AB_TRANSACTION_LIMITS *limits=AB_AccountSpec_GetTransactionLimitsForCommand(account,cmd);
  if(!limits) {
    unsupported_account_job=1;
    progress_log(NULL,0,GWEN_LoggerLevel_Warning,cmd==AB_Transaction_CommandGetTransactions?
        "AqBanking kann mit den aktuellen Kontodaten keinen Umsatzabruf für dieses Konto bereitstellen. Bitte den Bankzugang synchronisieren.":
        "AqBanking kann mit den aktuellen Kontodaten diesen Auftrag für das Konto nicht bereitstellen. Bitte den Bankzugang synchronisieren.");
    AB_AccountSpec_free(account); return GWEN_ERROR_NOT_SUPPORTED;
  }
  AB_TRANSACTION *t=NULL;
  if(cmd==AB_Transaction_CommandSepaDeleteStandingOrder) {
    const BC_FIELD *native=bc_field(&request,"native");
    if(!native || !native->size) { AB_AccountSpec_free(account); return GWEN_ERROR_INVALID; }
    GWEN_DB_NODE *db=GWEN_DB_Group_new("transaction");
    rv=GWEN_DB_ReadFromString(db,(const char *)native->value,native->size,BC_DB_READ_FLAGS);
    if(rv>=0) t=AB_Transaction_fromDb(db);
    GWEN_DB_Group_free(db);
    if(!t) { AB_AccountSpec_free(account); return GWEN_ERROR_INVALID; }
    // The object was obtained from the bank and stored inside this vault.
    // Still bind deletion to the exact account and order shown to the user.
    if(strcmp(str(AB_Transaction_GetLocalIban(t)),str(AB_AccountSpec_GetIban(account))) ||
       strcmp(str(AB_Transaction_GetFiId(t)),bc_text(&request,"order_id")) ||
       !*bc_text(&request,"order_id") ||
       strcmp(str(AB_Transaction_GetRemoteIban(t)),bc_text(&request,"iban")) ||
       strcmp(str(AB_Transaction_GetRemoteName(t)),bc_text(&request,"name"))) {
      rv=GWEN_ERROR_INVALID; goto cleanup;
    }
  } else t=AB_Transaction_new();
  AB_Banking_FillTransactionFromAccountSpec(t,account);
  AB_Transaction_SetCommand(t,cmd); AB_Transaction_SetUniqueId(t,AB_Banking_ReserveJobId(ab));
  if(*bc_text(&request,"from")) { GWEN_DATE *d=GWEN_Date_fromString(bc_text(&request,"from")); if(!d) {rv=GWEN_ERROR_INVALID;goto cleanup;} AB_Transaction_SetFirstDate(t,d); GWEN_Date_free(d); }
  if(*bc_text(&request,"to")) { GWEN_DATE *d=GWEN_Date_fromString(bc_text(&request,"to")); if(!d) {rv=GWEN_ERROR_INVALID;goto cleanup;} AB_Transaction_SetLastDate(t,d); GWEN_Date_free(d); }
  if(cmd>=1536 && cmd!=AB_Transaction_CommandSepaDeleteStandingOrder) {
    AB_Transaction_SetType(t,AB_Transaction_TypeTransfer);
    AB_Transaction_SetSubType(t,AB_Transaction_SubTypeStandard);
    AB_Transaction_SetRemoteName(t,bc_text(&request,"name")); AB_Transaction_SetRemoteIban(t,bc_text(&request,"iban"));
    AB_Transaction_SetRemoteBic(t,bc_text(&request,"bic")); AB_Transaction_SetPurpose(t,bc_text(&request,"purpose"));
    AB_Transaction_SetEndToEndReference(t,bc_text(&request,"end_to_end")); AB_Transaction_SetFiId(t,bc_text(&request,"order_id"));
    AB_VALUE *value=AB_Value_fromString(bc_text(&request,"amount"));
    if(!value) {rv=GWEN_ERROR_INVALID;goto cleanup;}
    AB_Value_SetCurrency(value,"EUR"); AB_Transaction_SetValue(t,value); AB_Value_free(value);
    if(!strcmp(bc_text(&request,"instant"),"1")) AB_Transaction_SetSubType(t,AB_Transaction_SubTypeEuASAP);
    const char *date_keys[]={"date","first","last"};
    for(unsigned i=0;i<3;i++) if(*bc_text(&request,date_keys[i])) {
      GWEN_DATE *d=GWEN_Date_fromString(bc_text(&request,date_keys[i])); if(!d) {rv=GWEN_ERROR_INVALID;goto cleanup;}
      if(i==0) AB_Transaction_SetDate(t,d); else if(i==1) AB_Transaction_SetFirstDate(t,d); else AB_Transaction_SetLastDate(t,d);
      GWEN_Date_free(d);
    }
    AB_Transaction_SetPeriod(t,!strcmp(bc_text(&request,"period"),"weekly")?AB_Transaction_PeriodWeekly:AB_Transaction_PeriodMonthly);
    AB_Transaction_SetCycle(t,(uint32_t)atoi(bc_text(&request,"cycle"))); AB_Transaction_SetExecutionDay(t,(uint32_t)atoi(bc_text(&request,"day")));
    rv=AB_Banking_CheckTransactionForSepaConformity(t,0);
    if(rv<0) goto cleanup;
    rv=AB_Banking_CheckTransactionAgainstLimits_Names(t,limits); if(rv<0) goto cleanup;
    rv=AB_Banking_CheckTransactionAgainstLimits_Purpose(t,limits); if(rv<0) goto cleanup;
    if(cmd==AB_Transaction_CommandSepaCreateStandingOrder) {
      rv=AB_Banking_CheckTransactionAgainstLimits_Recurrence(t,limits); if(rv<0) goto cleanup;
      rv=AB_Banking_CheckTransactionAgainstLimits_ExecutionDate(t,limits); if(rv<0) goto cleanup;
    }
    if(cmd==AB_Transaction_CommandSepaCreateDatedTransfer) {
      rv=AB_Banking_CheckTransactionAgainstLimits_Date(t,limits); if(rv<0) goto cleanup;
    }
  }
  AB_TRANSACTION_LIST2 *commands=AB_Transaction_List2_new(); AB_Transaction_List2_PushBack(commands,t);
  AB_IMEXPORTER_CONTEXT *context=AB_ImExporterContext_new();
  rv=card_session?send_card_read_with(ab,account,t,context,AB_Provider_SendCommands):
                  AB_Banking_SendCommands(ab,commands,context);
  int status=AB_Transaction_GetStatus(t);
  // An outer success is not proof that an individual payment was accepted.
  if(rv>=0 && status!=AB_Transaction_StatusAccepted) rv=GWEN_ERROR_GENERIC;
  if(rv>=0) for(AB_IMEXPORTER_ACCOUNTINFO *a=AB_ImExporterContext_GetFirstAccountInfo(context);a;a=AB_ImExporterAccountInfo_List_Next(a)) {
    if(AB_ImExporterAccountInfo_GetAccountId(a)!=AB_AccountSpec_GetUniqueId(account)) continue;
    rv=emit_account_results(a);
    if(rv<0) break; // Parent applies the batch only after a successful done frame.
  }
  AB_ImExporterContext_free(context); AB_Transaction_List2_free(commands);
cleanup:
  AB_Transaction_free(t); AB_AccountSpec_free(account); return rv;
}

#include "cards.h"

static int read_operation(const char *op) {
  return !strcmp(op,"open") || !strcmp(op,"accounts") || !strcmp(op,"balance") || !strcmp(op,"transactions") || !strcmp(op,"read_batch");
}
static int session_request_allowed(const BC_FRAME *next) {
  const char *op=bc_text(next,"op");
  if(!read_operation(op) && strcmp(op,"close")) return 0;
  for(unsigned i=0;i<next->count;i++) {
    const char *key=next->fields[i].key;
    if(!strcmp(op,"read_batch")) {if(!read_batch_key(key)) return 0; continue;}
    if(strcmp(key,"op") && strcmp(key,"account") && strcmp(key,"from") && strcmp(key,"to")) return 0;
  }
  return 1;
}
static int execute_request(AB_BANKING *ab) {
  int rv=0;
  const char *op=bc_text(&request,"op");
  const char *operations[]={"probe","read_card","chiptan_usb","configure","open","accounts","reset_tan","balance","transactions","read_batch","standing","transfer","scheduled","create_standing","delete_standing"};
  int known=0;
  for(unsigned i=0;i<sizeof(operations)/sizeof(operations[0]);i++) if(!strcmp(op,operations[i])) known=1;
  if(!known) rv=GWEN_ERROR_INVALID;
  else if(!strcmp(op,"probe")) rv=0;
  else if(!strcmp(op,"read_card")) rv=read_card_context();
  else if(!strcmp(op,"chiptan_usb")) rv=generate_tan();
  else {
    rv=ensure_user(ab);
    if(rv==0) rv=upgrade_snapshot(ab);
    if(rv==0 && !strcmp(op,"configure")) { /* offline setup only */ }
    // Editing a wrong medium must work even when it prevents getaccounts.
    // Use the methods already reported by the bank; no successful login is
    // required just to correct the local TAN configuration.
    else if(rv==0 && !strcmp(op,"reset_tan")) rv=select_tan(ab,1);
    else if(rv==0) {
      int accounts_fetched=0;
      rv=initialize_access(ab,control,select_tan,&accounts_fetched);
      if(rv==0 && !strcmp(op,"accounts") && !accounts_fetched) {
        GWEN_DB_DeleteVar(bc_config,"sepa_initialized");
        rv=control(ab,"getaccounts"); accounts_fetched=(rv==0);
      }
      if(rv==0) rv=ensure_sepa_accounts(ab,accounts_fetched,get_sepa_accounts);
      if(rv==0) rv=prefer_camt_accounts(ab);
      if(rv==0) {
        begin_stage(op);
        // Export the freshly rebuilt capabilities even when a selected job
        // is unsupported. The parent must not keep pre-migration CAMT flags.
        rv=emit_accounts(ab);
      }
      if(rv==0) {
        if(!strcmp(op,"accounts") || !strcmp(op,"open")) rv=0;
        else if(!strcmp(op,"read_batch")) rv=run_read_batch_with(ab,AB_Banking_GetAccountSpecByUniqueId,AB_Provider_SendCommands);
        else if(!strcmp(op,"balance")) rv=run_job(ab,AB_Transaction_CommandGetBalance);
        else if(!strcmp(op,"transactions")) rv=run_job(ab,AB_Transaction_CommandGetTransactions);
        else if(!strcmp(op,"standing")) rv=run_job(ab,AB_Transaction_CommandSepaGetStandingOrders);
        else if(!strcmp(op,"transfer")) rv=run_job(ab,AB_Transaction_CommandSepaTransfer);
        else if(!strcmp(op,"scheduled")) rv=run_job(ab,AB_Transaction_CommandSepaCreateDatedTransfer);
        else if(!strcmp(op,"create_standing")) rv=run_job(ab,AB_Transaction_CommandSepaCreateStandingOrder);
        else if(!strcmp(op,"delete_standing")) rv=run_job(ab,AB_Transaction_CommandSepaDeleteStandingOrder);
        else rv=GWEN_ERROR_NOT_SUPPORTED;
      }
    }
  }
  return rv;
}
static void emit_done(int rv,int reusable) {
  BC_FRAME result=event("done"); bc_number(&result,"code",rv); bc_number(&result,"cancelled",was_cancelled);
  bc_number(&result,"bad_pin",bad_pin); bc_number(&result,"unsupported_account_job",unsupported_account_job);
  bc_number(&result,"session",reusable);
  bc_string(&result,"stage",operation_stage); bc_string(&result,"notice",last_notice);
  emit_db(&result,"state",bc_config); bc_send(&result);
}

static int valid_product_identity(const BC_FRAME *frame) {
  const char *product=bc_text(frame,"product");
  if(strlen(product)!=25) return 0;
  for(const char *p=product;*p;p++)
    if(!((*p>='A' && *p<='Z') || (*p>='a' && *p<='z') || (*p>='0' && *p<='9'))) return 0;
  const char *version=bc_text(frame,"product_version");
  if(!*version || strlen(version)>5) return 0;
  for(const char *p=version;*p;p++)
    if(!((*p>='0' && *p<='9') || *p=='.')) return 0;
  return 1;
}

int main(int argc,char **argv) {
  (void)argc;(void)argv;
#ifdef _WIN32
  _setmode(_fileno(stdin),_O_BINARY); _setmode(_fileno(stdout),_O_BINARY);
#else
  struct rlimit core={0,0}; setrlimit(RLIMIT_CORE,&core);
#endif
  bc_output=fdopen(dup(fileno(stdout)),"wb");
  // Native diagnostics can contain banking messages. The private IPC descriptor
  // is the sole output channel; libraries cannot accidentally corrupt it.
  if(!bc_output || !freopen(BC_NULL,"wb",stdout) || !freopen(BC_NULL,"wb",stderr)) return 71;
  if(!bc_read(&request) || strcmp(bc_text(&request,"version"),"1")) return 71;
  // Reject missing, invalid or overlong identities before any banking work.
  if(!valid_product_identity(&request)) return 71;
  if(AB_BankingCraft_NoLegacyImport()!=1) return 71;
  if(AB_BankingCraft_HkvvbOnlyRegistration()!=1) return 71;
  if(GWEN_Init()<0) return 71;
  GWEN_GUI *gui=setup_gui();
  bc_config=GWEN_DB_Group_new("bankingcraft");
  const BC_FIELD *snapshot=bc_field(&request,"state");
  if(snapshot && snapshot->size && GWEN_DB_ReadFromString(bc_config,(const char *)snapshot->value,snapshot->size,BC_DB_READ_FLAGS)<0) return 71;
  if(bc_install_memory_config()<0) return 71;
  struct stat st; const char *data_path=bc_text(&request,"data_path");
  // A regular-file blocker makes AqBanking's separate raw-message log path
  // unwritable. No account information is ever written into this file.
  if(stat(data_path,&st) || !S_ISREG(st.st_mode)) return 71;
  AB_BANKING *ab=AB_Banking_new("BankingCraft",data_path,0);
  // Patched AqBanking uses this value only in HKVVB/Produktbezeichnung.
  AB_Banking_RuntimeConfig_SetCharValue(ab,"fintsRegistrationKey",bc_text(&request,"product"));
  AB_Banking_RuntimeConfig_SetCharValue(ab,"fintsApplicationVersionString",bc_text(&request,"product_version"));
  AB_Banking_RuntimeConfig_SetIntValue(ab,"bankingcraftSkipMatchingVop",
      !strcmp(bc_text(&request,"skip_matching_vop"),"1"));
  int rv=AB_Banking_Init(ab);
  if(rv>=0) {
    BC_FRAME connection={0};
    const char *keys[]={"blz","user","customer","url","medium","record"};
    for(unsigned i=0;i<sizeof(keys)/sizeof(keys[0]);i++) bc_string(&connection,keys[i],bc_text(&request,keys[i]));
    card_session=!strcmp(bc_text(&request,"card_session"),"1");
    if(card_session && (strcmp(bc_text(&request,"medium"),"zkacard") && strcmp(bc_text(&request,"medium"),"ddvcard")))
      rv=GWEN_ERROR_INVALID;
    unsigned count=0;
    int emitted=0;
    while(rv==0) {
      if(card_session && !read_operation(bc_text(&request,"op"))) rv=GWEN_ERROR_INVALID;
      else rv=execute_request(ab);
      // Only a local capability rejection may continue. Authentication errors,
      // device removal, cancellation and bank failures close the read session.
      int reusable=card_session && !was_cancelled && !bad_pin && !certificate_rejected &&
          (rv==0 || (rv==GWEN_ERROR_NOT_SUPPORTED && unsupported_account_job));
      if(!reusable) break;
      emit_done(rv,1); emitted=1;
      BC_FRAME next={0};
      if(!bc_read(&next)) break; // parent closed normally or disappeared
      bc_free(&request); request=next;
      if(!session_request_allowed(&request) || ++count>1024) { rv=GWEN_ERROR_INVALID; emitted=0; break; }
      if(!strcmp(bc_text(&request,"op"),"close")) break;
      for(unsigned i=0;i<connection.count;i++) bc_add(&request,connection.fields[i].key,connection.fields[i].value,connection.fields[i].size);
      was_cancelled=bad_pin=certificate_rejected=unsupported_account_job=0;
      begin_stage("initialize"); rv=0; emitted=0;
    }
    bc_free(&connection);
    int fini=AB_Banking_Fini(ab); if(rv==0 && fini<0) rv=fini;
    if(!emitted) emit_done(rv,0);
  } else emit_done(rv,0);
  AB_Banking_free(ab);
  GWEN_DB_Group_free(bc_config); bc_free(&request); bc_wipe(last_notice,sizeof(last_notice));
  GWEN_Gui_SetGui(NULL); GWEN_Gui_free(gui); GWEN_Fini(); fclose(bc_output); return 0;
}
