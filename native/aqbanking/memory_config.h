/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef BANKINGCRAFT_MEMORY_CONFIG_H
#define BANKINGCRAFT_MEMORY_CONFIG_H

#include <gwenhywfar/configmgr_be.h>
#include <gwenhywfar/db.h>
#include <gwenhywfar/error.h>

/* The helper has exactly one access and one owner. AqBanking's configuration
 * manager is replaced before AB_Banking_new; all groups live in this tree.
 * The parent encrypts snapshots in its vault. Never use a directory backend. */
static GWEN_DB_NODE *bc_config;
// Native config contains repeated groups (updjob, tanMethod, transactions...).
// DEFAULT alone merges them into one group with multi-valued fields, which
// changes AqBanking's interpretation. Match the native directory reader.
#define BC_DB_READ_FLAGS (GWEN_DB_FLAGS_DEFAULT | GWEN_PATH_FLAGS_CREATE_GROUP)

static GWEN_DB_NODE *bc_group(const char *group, const char *sub, int create) {
  char path[1024];
  if(!group || !sub || strchr(group,'/') || strchr(sub,'/') ||
     snprintf(path,sizeof(path),"groups/%s/%s",group,sub)>=(int)sizeof(path)) return NULL;
  return GWEN_DB_GetGroup(bc_config, create?GWEN_DB_FLAGS_DEFAULT:GWEN_PATH_FLAGS_NAMEMUSTEXIST,path);
}
static int bc_get_group(GWEN_CONFIGMGR *mgr,const char *g,const char *s,GWEN_DB_NODE **out) {
  (void)mgr; GWEN_DB_NODE *db=bc_group(g,s,0);
  *out=db?GWEN_DB_Group_dup(db):GWEN_DB_Group_new(s); return *out?0:GWEN_ERROR_MEMORY_FULL;
}
static int bc_set_group(GWEN_CONFIGMGR *mgr,const char *g,const char *s,GWEN_DB_NODE *db) {
  (void)mgr; GWEN_DB_NODE *dest=bc_group(g,s,1);
  if(!dest) return GWEN_ERROR_INVALID;
  GWEN_DB_ClearGroup(dest,NULL);
  return GWEN_DB_AddGroupChildren(dest,db);
}
static int bc_has_group(GWEN_CONFIGMGR *mgr,const char *g,const char *s) {
  (void)mgr; return bc_group(g,s,0)?0:GWEN_ERROR_NOT_FOUND;
}
static int bc_lock_group(GWEN_CONFIGMGR *mgr,const char *g,const char *s) {
  (void)mgr;(void)g;(void)s; return 0;
}
static int bc_delete_group(GWEN_CONFIGMGR *mgr,const char *g,const char *s) {
  (void)mgr; GWEN_DB_NODE *db=bc_group(g,s,0);
  if(!db) return GWEN_ERROR_NOT_FOUND;
  GWEN_DB_UnlinkGroup(db); GWEN_DB_Group_free(db); return 0;
}
static int bc_list_sub(GWEN_CONFIGMGR *mgr,const char *g,GWEN_STRINGLIST *sl) {
  (void)mgr; char path[1024];
  if(!g || strchr(g,'/') || snprintf(path,sizeof(path),"groups/%s",g)>=(int)sizeof(path)) return GWEN_ERROR_INVALID;
  GWEN_DB_NODE *db=GWEN_DB_GetGroup(bc_config,GWEN_PATH_FLAGS_NAMEMUSTEXIST,path);
  if(db) for(db=GWEN_DB_GetFirstGroup(db);db;db=GWEN_DB_GetNextGroup(db))
    GWEN_StringList_AppendString(sl,GWEN_DB_GroupName(db),0,1);
  return 0;
}
static int bc_list_groups(GWEN_CONFIGMGR *mgr,GWEN_STRINGLIST *sl) {
  (void)mgr; GWEN_DB_NODE *db=GWEN_DB_GetGroup(bc_config,GWEN_PATH_FLAGS_NAMEMUSTEXIST,"groups");
  if(db) for(db=GWEN_DB_GetFirstGroup(db);db;db=GWEN_DB_GetNextGroup(db))
    GWEN_StringList_AppendString(sl,GWEN_DB_GroupName(db),0,1);
  return 0;
}
static int bc_id(GWEN_CONFIGMGR *mgr,const char *g,uint32_t id,int check,char *out,uint32_t size) {
  (void)mgr;
  if(size<9) return GWEN_ERROR_BUFFER_OVERFLOW;
  snprintf(out,size,"%08x",id);
  if(check && bc_group(g,out,0)) return GWEN_ERROR_FOUND;
  return 0;
}
static int bc_next_id(GWEN_CONFIGMGR *mgr,const char *g,char *out,uint32_t size) {
  if(size<14) return GWEN_ERROR_BUFFER_OVERFLOW;
  uint32_t id=(uint32_t)GWEN_DB_GetIntValue(bc_config,"counter",0,0);
  char name[32];
  do { if(++id>INT32_MAX) return GWEN_ERROR_BUFFER_OVERFLOW; bc_id(mgr,g,id,0,name,sizeof(name)); } while(bc_group(g,name,0));
  GWEN_DB_SetIntValue(bc_config,GWEN_DB_FLAGS_OVERWRITE_VARS,"counter",(int)id);
  snprintf(out,size,"uid::%08x",id); return 0;
}
static GWEN_CONFIGMGR *bc_config_factory(GWEN_PLUGIN *plugin,const char *url) {
  (void)plugin; GWEN_CONFIGMGR *m=GWEN_ConfigMgr_new(url);
  GWEN_ConfigMgr_SetGetGroupFn(m,bc_get_group);
  GWEN_ConfigMgr_SetSetGroupFn(m,bc_set_group);
  GWEN_ConfigMgr_SetHasGroupFn(m,bc_has_group);
  GWEN_ConfigMgr_SetLockGroupFn(m,bc_lock_group);
  GWEN_ConfigMgr_SetUnlockGroupFn(m,bc_lock_group);
  GWEN_ConfigMgr_SetDeleteGroupFn(m,bc_delete_group);
  GWEN_ConfigMgr_SetListGroupsFn(m,bc_list_groups);
  GWEN_ConfigMgr_SetListSubGroupsFn(m,bc_list_sub);
  GWEN_ConfigMgr_SetGetUniqueIdFn(m,bc_next_id);
  GWEN_ConfigMgr_SetMkUniqueIdFromIdFn(m,bc_id);
  return m;
}
static int bc_install_memory_config(void) {
  GWEN_PLUGIN_MANAGER *pm=GWEN_PluginManager_FindPluginManager(GWEN_CONFIGMGR_PLUGIN_NAME);
  if(!pm) return GWEN_ERROR_NOT_FOUND;
  GWEN_PLUGIN *p=GWEN_ConfigMgr_Plugin_new(pm,"dir",NULL);
  if(!p) return GWEN_ERROR_MEMORY_FULL;
  GWEN_ConfigMgr_Plugin_SetFactoryFn(p,bc_config_factory);
  GWEN_PluginManager_AddPlugin(pm,p);
  return 0;
}
#endif
