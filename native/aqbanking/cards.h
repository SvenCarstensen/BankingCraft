/* SPDX-License-Identifier: GPL-3.0-only
 * Card operations are delegated to Libchipcard. No APDUs or private keys cross
 * the application boundary. The parent bounds and validates the returned TAN.
 */
static int match_card_context(GWEN_CRYPT_TOKEN *token,const char *bank,const char *user,uint32_t *selected) {
  *selected=0;
  if(!*bank || !*user) return GWEN_ERROR_INVALID;
  uint32_t ids[32],count=32;
  int rv=GWEN_Crypt_Token_GetContextIdList(token,ids,&count,0);
  if(rv!=0) return rv;
  if(count>32) return GWEN_ERROR_BAD_DATA;
  for(uint32_t i=0;i<count;i++) {
    if(!ids[i]) return GWEN_ERROR_BAD_DATA;
    const GWEN_CRYPT_TOKEN_CONTEXT *context=GWEN_Crypt_Token_GetContext(token,ids[i],0);
    if(!context) return was_cancelled?GWEN_ERROR_USER_ABORTED:GWEN_ERROR_BAD_DATA;
    if(strcmp(str(GWEN_Crypt_Token_Context_GetServiceId(context)),bank) ||
       strcmp(str(GWEN_Crypt_Token_Context_GetUserId(context)),user)) continue;
    if(*selected) {
      *selected=0;
      progress_log(NULL,0,GWEN_LoggerLevel_Error,
          "Mehrere Einträge auf der Chipkarte passen zum Bankzugang. Bitte den gewünschten Karteneintrag ausdrücklich auswählen.");
      return GWEN_ERROR_INVALID;
    }
    *selected=ids[i];
  }
  if(!*selected) {
    progress_log(NULL,0,GWEN_LoggerLevel_Error,
        "Auf der Chipkarte wurde kein Eintrag mit der Bankleitzahl und Benutzerkennung dieses Bankzugangs gefunden.");
    return GWEN_ERROR_NOT_FOUND;
  }
  return 0;
}

static int resolve_auto_card_context(uint32_t *selected,GWEN_BUFFER *token_name) {
  GWEN_PLUGIN_MANAGER *manager=GWEN_PluginManager_FindPluginManager("ct");
  GWEN_PLUGIN *plugin=manager?GWEN_PluginManager_GetPlugin(manager,bc_text(&request,"medium")):NULL;
  GWEN_CRYPT_TOKEN *token=plugin?GWEN_Crypt_Token_Plugin_CreateToken(plugin,NULL):NULL;
  if(!token) return GWEN_ERROR_NOT_FOUND;
  int rv=GWEN_Crypt_Token_Open(token,0,0);
  if(rv==0) {
    rv=match_card_context(token,bc_text(&request,"blz"),bc_text(&request,"user"),selected);
    if(rv==0) {
      const char *name=GWEN_Crypt_Token_GetTokenName(token);
      if(!name || !*name) rv=GWEN_ERROR_BAD_DATA;
      else GWEN_Buffer_AppendString(token_name,name);
    }
    int closed=GWEN_Crypt_Token_Close(token,rv!=0,0);
    if(rv==0) rv=closed;
  }
  GWEN_Crypt_Token_free(token);
  if(rv!=0 && !*last_notice && !was_cancelled)
    progress_log(NULL,0,GWEN_LoggerLevel_Error,
        "Libchipcard konnte die Bankeinträge der Chipkarte nicht lesen. Bitte die HBCI-Karte im ausgewählten Leser prüfen.");
  return rv;
}

static int read_card_context(void) {
  const char *kind=bc_text(&request,"medium");
  if(strcmp(kind,"ddvcard") && strcmp(kind,"zkacard")) return GWEN_ERROR_INVALID;
  GWEN_PLUGIN_MANAGER *manager=GWEN_PluginManager_FindPluginManager("ct");
  GWEN_PLUGIN *plugin=manager?GWEN_PluginManager_GetPlugin(manager,kind):NULL;
  if(!plugin) return GWEN_ERROR_NOT_FOUND;
  GWEN_CRYPT_TOKEN *token=GWEN_Crypt_Token_Plugin_CreateToken(plugin,NULL);
  if(!token) return GWEN_ERROR_NOT_FOUND;
  int rv=GWEN_Crypt_Token_Open(token,0,0);
  if(rv==0) {
    uint32_t ids[32], count=32;
    rv=GWEN_Crypt_Token_GetContextIdList(token,ids,&count,0);
    if(count>32) rv=GWEN_ERROR_INVALID;
    uint32_t selected=(uint32_t)strtoul(bc_text(&request,"record"),NULL,10);
    int found=0;
    if(rv==0) for(uint32_t i=0;i<count;i++) {
      if(selected && selected!=ids[i]) continue;
      const GWEN_CRYPT_TOKEN_CONTEXT *c=GWEN_Crypt_Token_GetContext(token,ids[i],0);
      if(!c) { rv=GWEN_ERROR_NOT_FOUND; break; }
      BC_FRAME f=event("card");
      bc_number(&f,"record",ids[i]); bc_number(&f,"profile",GWEN_Crypt_Token_Context_GetProtocolVersion(c));
      bc_string(&f,"number",GWEN_Crypt_Token_GetTokenName(token));
      bc_string(&f,"blz",GWEN_Crypt_Token_Context_GetServiceId(c));
      bc_string(&f,"bank_name",GWEN_Crypt_Token_Context_GetPeerName(c));
      bc_string(&f,"user",GWEN_Crypt_Token_Context_GetUserId(c));
      bc_string(&f,"customer",GWEN_Crypt_Token_Context_GetCustomerId(c));
      bc_string(&f,"url",GWEN_Crypt_Token_Context_GetAddress(c));
      bc_send(&f); found=1; break;
    }
    if(rv==0 && !found) rv=GWEN_ERROR_NOT_FOUND;
    int closed=GWEN_Crypt_Token_Close(token,rv!=0,0);
    if(rv==0) rv=closed;
  }
  GWEN_Crypt_Token_free(token); return rv;
}

static int generate_tan(void) {
  const BC_FIELD *payload=bc_field(&request,"payload");
  if(!payload || !payload->size || payload->size>247) return GWEN_ERROR_INVALID;
  LC_CLIENT *client=LC_Client_new("BankingCraft","1");
  if(!client) return GWEN_ERROR_MEMORY_FULL;
  LC_CARD *card=NULL; int opened=0, started=0, initialized=0, rv=GWEN_ERROR_NOT_CONNECTED;
  GWEN_BUFFER *response=GWEN_Buffer_new(NULL,256,0,1);
  unsigned char command[255]={0,0,0,0,1,0,0,0};
  command[7]=(unsigned char)payload->size; memcpy(command+8,payload->value,payload->size);
  if(LC_Client_Init(client)!=0) goto done;
  initialized=1;
  if(LC_Client_Start(client)!=LC_Client_ResultOk) goto done;
  started=1;
  if(LC_Client_GetNextCard(client,&card,20)!=LC_Client_ResultOk) goto done;
  if(LC_ChiptanusbCard_ExtendCard(card)!=0 || LC_Card_Open(card)!=LC_Client_ResultOk) goto done;
  opened=1;
  if(LC_ChiptanusbCard_GenerateTan(card,command,(int)payload->size+8,response)!=LC_Client_ResultOk) goto done;
  if(GWEN_Buffer_GetUsedBytes(response)>4096) { rv=GWEN_ERROR_INVALID; goto done; }
  BC_FRAME f=event("tan"); bc_add(&f,"response",GWEN_Buffer_GetStart(response),GWEN_Buffer_GetUsedBytes(response)); bc_send(&f); rv=0;
done:
  if(opened) LC_Card_Close(card);
  if(card) { LC_Client_ReleaseCard(client,card); LC_Card_free(card); }
  if(started) LC_Client_Stop(client);
  if(initialized) LC_Client_Fini(client);
  LC_Client_free(client); bc_wipe(command,sizeof(command));
  bc_wipe(GWEN_Buffer_GetStart(response),GWEN_Buffer_GetUsedBytes(response)); GWEN_Buffer_free(response);
  return rv;
}
