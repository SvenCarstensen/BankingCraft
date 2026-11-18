#!/usr/bin/env python3
"""Build pinned AqBanking/Libchipcard sources and BankingCraft's isolated worker.

Prerequisites: C/C++ compiler, make, pkg-config, GnuTLS, libgcrypt, GMP,
libxml2/xmlsec, gettext, and PC/SC development headers (macOS: system SDK).
Windows: run in an MSYS2 UCRT64 environment; the Rust executable may use MSVC.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import subprocess
import sys
import tarfile
import urllib.request

ROOT = Path(__file__).resolve().parents[1]

def replace_once(path, old, new):
    content = path.read_text()
    if new in content:
        return
    if content.count(old) != 1:
        raise RuntimeError(f"Upstream source changed: {path.name}; review the patch")
    path.write_text(content.replace(old, new))

def patch_source(name, source):
    if name == "gwenhywfar":
        # Pinned Gwen reports reader PIN entry only through progress logs, which
        # are hidden during account refresh. Emit explicit begin/end UI events.
        replace_once(source / "src/crypttoken/ct.c",
                     '''  int rv;

  rv=GWEN_Gui_ProgressLog(gid, GWEN_LoggerLevel_Warning, I18N("Waiting for pin entry on card reader..."));
  if (rv<0)
    return 0;
  return 0xffffffff;''',
                     '''  /* BankingCraft: typed reader PIN notification, never collect the PIN. */
  return GWEN_Gui_ShowBox(GWEN_GUI_SHOWBOX_FLAGS_BEEP,
                          "BankingCraft:reader-pin",
                          "Bitte geben Sie die PIN Ihrer HBCI-Chipkarte am Kartenleser ein und bestätigen Sie mit OK.", gid);''')
        replace_once(source / "src/crypttoken/ct.c",
                     '  return GWEN_Gui_ProgressLog(gid, GWEN_LoggerLevel_Warning, I18N("Pin entry on card reader finished."));',
                     '  GWEN_Gui_HideBox(gid);\n  return 0;')
    if name == "aqbanking":
        import importlib.util
        spec=importlib.util.spec_from_file_location("patch_instant", ROOT/"scripts/patch-instant.py")
        instant=importlib.util.module_from_spec(spec); spec.loader.exec_module(instant)
        instant.patch(source, replace_once)
        # Linux LeakSanitizer exposes ownership bugs in pinned 6.9.2.
        # Free temporary objects at their owners; keep leak detection enabled.
        replace_once(source / "src/libs/plugins/backends/aqhbci/admjobs/vop_result.c",
                     '    GWEN_LIST_FINI(AH_VOP_RESULT, vr);\n    GWEN_FREE_OBJECT(vr);',
                     '    GWEN_LIST_FINI(AH_VOP_RESULT, vr);\n'
                     '    free(vr->localBic);\n    free(vr->remoteIban);\n'
                     '    free(vr->remoteName);\n    free(vr->altRemoteName);\n'
                     '    GWEN_FREE_OBJECT(vr);')
        replace_once(source / "src/libs/plugins/backends/aqhbci/joblayer/jobqueue_account.c",
                     '    accountSpecList=AB_AccountSpec_List_new();\n    rv=AB_Banking_GetAccountSpecList(ab, &accountSpecList);',
                     '    /* GetAccountSpecList allocates the returned list. */\n    rv=AB_Banking_GetAccountSpecList(ab, &accountSpecList);')
        replace_once(source / "src/libs/aqbanking/banking_imex.c",
                     '  } /* while(seGlobalDataDir) */\n  GWEN_Buffer_free(pathBuffer);',
                     '  } /* while(seGlobalDataDir) */\n  GWEN_StringList_free(slGlobalDataDirs);\n  GWEN_Buffer_free(pathBuffer);')
        replace_once(source / "src/libs/plugins/backends/aqhbci/joblayer/job_swift.c",
                     '      DBG_INFO(AQHBCI_LOGDOMAIN, "Param [%s] does not match family %s.%d", sName, family, version1);\n    }',
                     '      DBG_INFO(AQHBCI_LOGDOMAIN, "Param [%s] does not match family %s.%d", sName, family, version1);\n    }\n    AB_SwiftDescr_free(tmpDescr);')
        replace_once(source / "src/libs/plugins/backends/aqhbci/joblayer/job_swift.c",
                     '          DBG_INFO(AQHBCI_LOGDOMAIN, "Param [%s] does not match family %s.%03d", s, family, version1);\n        }',
                     '          DBG_INFO(AQHBCI_LOGDOMAIN, "Param [%s] does not match family %s.%03d", s, family, version1);\n        }\n        AB_SwiftDescr_free(tmpDescr);')
        replace_once(source / "src/libs/plugins/backends/aqhbci/ajobs/jobtransferbase.c",
                     '  free(aj->profileName);\n  free(aj->fiid);',
                     '  free(aj->profileName);\n  free(aj->descriptor);\n  free(aj->fiid);')
        # Export groups own temporary metadata and only borrow transactions.
        # Release them after copying to the DB, including message-ID failure.
        path = source / "src/libs/plugins/imexporters/xml/xml_sepa_exp.c"
        replace_once(path,
                     '    DBG_ERROR(AQBANKING_LOGDOMAIN, "Could not generate message id, aborting.");\n    GWEN_DB_Group_free(dbData);',
                     '    DBG_ERROR(AQBANKING_LOGDOMAIN, "Could not generate message id, aborting.");\n    AB_ImExporterXML_PaymentGroup_List_free(paymentGroupList);\n    GWEN_DB_Group_free(dbData);')
        replace_once(path,
                     '  _writePaymentGroups(paymentGroupList, dbData, dbParams);\n\n  return dbData;',
                     '  _writePaymentGroups(paymentGroupList, dbData, dbParams);\n  AB_ImExporterXML_PaymentGroup_List_free(paymentGroupList);\n\n  return dbData;')
        # camt.052.001.08 creates one transaction per Ntry, but 6.9.2 reads
        # its amount inside optional NtryDtls/TxDtls. Banks can omit those
        # details, and a detail/foreign-currency amount is not the booked
        # entry amount. Keep this correction inside AqBanking's XML profile.
        camt = source / "src/libs/plugins/imexporters/xml/data/camt_052_001_08.xml"
        for sign, direction in [("-", "DBIT"), ("", "CRDT")]:
            replace_once(camt,
                         '''                        <XmlIfHasCharData path="Amt">
                          <XmlIfHasCharData path="Amt@Ccy">
                            <DbCreateAndEnterTempGroup name="amount" >
                              <DbSetTempCharValue name="value"    path="Amt" />
                              <DbSetTempCharValue name="currency" path="Amt@Ccy" />
                              <DbSetCharValue name="value" value="''' + sign + '''$(value):$(currency)" />
                            </DbCreateAndEnterTempGroup> <!-- amount -->
                          </XmlIfHasCharData> <!-- Amt@Ccy -->
                        </XmlIfHasCharData>   <!-- Amt -->''',
                         "                        <!-- " + direction + " amount is read from Ntry above. -->")
        amounts = "                  <!-- BankingCraft: booked entry amount, independent of optional details. -->\n"
        for sign, direction in [("-", "DBIT"), ("", "CRDT")]:
            amounts += '''                  <XmlIfCharDataMatches path="CdtDbtInd" pattern="''' + direction + '''">
                    <XmlIfHasCharData path="Amt@Ccy">
                      <DbCreateAndEnterTempGroup name="amount">
                        <DbSetTempCharValue name="value" path="Amt" />
                        <DbSetTempCharValue name="currency" path="Amt@Ccy" />
                        <DbSetCharValue name="value" value="''' + sign + '''$(value):$(currency)" mode="replace" />
                      </DbCreateAndEnterTempGroup>
                    </XmlIfHasCharData>
                  </XmlIfCharDataMatches>
'''
        replace_once(camt, "                  <!-- booking date -->", amounts + "\n                  <!-- booking date -->")
        # A legacy HKKAZ permission may survive in UPD after HIKAZS disappears
        # from BPD. The first advertised code must not hide a supported card
        # job later in the list. These are local factory attempts, not retries
        # of requests sent to the bank.
        replace_once(source / "src/libs/plugins/backends/aqhbci/ajobs/jobgettransactions.c",
                     '''  int jobType;

  jobType=_determineJobToUse(u, account);
  switch(jobType) {
  case AH_JOB_GETTRANS_HKKAZ: return _createJob_HKKAZ(pro, u, account);
  case AH_JOB_GETTRANS_DKKKU: return _createJob_DKKKU(pro, u, account);
  case AH_JOB_GETTRANS_HKKKU: return _createJob_HKKKU(pro, u, account);
  case AH_JOB_GETTRANS_HKWDU: return _createJob_HKWDU(pro, u, account);
  default:
    DBG_NOTICE(AQHBCI_LOGDOMAIN, "No FinTS job (GetTransaction) available for this account");
    return NULL;
  }''',
                     '''  GWEN_DB_NODE *updgroup=AH_User_GetUpdForAccount(u, account);
  GWEN_DB_NODE *n=updgroup?GWEN_DB_GetFirstGroup(updgroup):NULL;
  for (; n; n=GWEN_DB_GetNextGroup(n)) {
    const char *code=GWEN_DB_GetCharValue(n, "job", 0, "");
    AH_JOB *job=NULL;
    if (strcasecmp(code, "HKKAZ")==0)
      job=_createJob_HKKAZ(pro, u, account);
    else if (strcasecmp(code, "DKKKU")==0)
      job=_createJob_DKKKU(pro, u, account);
    else if (strcasecmp(code, "HKKKU")==0)
      job=_createJob_HKKKU(pro, u, account);
    else if (strcasecmp(code, "HKWDU")==0)
      job=_createJob_HKWDU(pro, u, account);
    if (job)
      return job;
  }
  DBG_NOTICE(AQHBCI_LOGDOMAIN, "No FinTS job (GetTransaction) available for this account");
  return NULL;''')
        # Upstream master already guards this automatic legacy import with
        # !dname. 6.9.2 must not read or copy another application's bank config.
        path = source / "src/libs/aqbanking/banking.c"
        replace_once(path, "  {\n    int rv;\n\n    rv=AB_Banking_CopyOldSettingsFolderIfNeeded(ab);",
                     "  if (!dname) {\n    int rv;\n\n    rv=AB_Banking_CopyOldSettingsFolderIfNeeded(ab);")
        content = path.read_text()
        if "AB_BankingCraft_NoLegacyImport" not in content:
            path.write_text(content + "\nint AB_BankingCraft_NoLegacyImport(void) { return 1; }\n")
        # Registration belongs exclusively to HKVVB. Upstream also uses the
        # registration accessor in INI letters and log/app-name metadata.
        # Keep the message-engine lookup unchanged; use the actual app name
        # everywhere else, including the public provider accessor.
        old = "AH_HBCI_GetProductName(h)"
        new = "AB_Banking_GetAppName(AH_HBCI_GetBankingApi(h))"
        for filename, count in [("banking/provider_iniletter.c", 4),
                                ("banking/provider.c", 1), ("msglayer/message.c", 1)]:
            path = source / "src/libs/plugins/backends/aqhbci" / filename
            content = path.read_text()
            if content.count(old) == count and new not in content:
                path.write_text(content.replace(old, new))
            elif old in content or content.count(new) != count:
                raise RuntimeError(f"Upstream product-name usage changed: {filename}")
        path = source / "src/libs/aqbanking/banking.c"
        content = path.read_text()
        if "AB_BankingCraft_HkvvbOnlyRegistration" not in content:
            path.write_text(content + "\nint AB_BankingCraft_HkvvbOnlyRegistration(void) { return 1; }\n")
        # Never create raw FinTS logs. Besides containing credentials, the
        # upstream failure branch frees its path buffer twice. Keep the regular
        # file blocker as defence for any other attempted filesystem writes.
        path = source / "src/libs/plugins/backends/aqhbci/msglayer/dialog.c"
        content = path.read_text()
        marker = "  /* BankingCraft: raw banking logs disabled. */"
        if marker not in content:
            start = content.index("  /* create path */")
            end = content.index("\n  return dlg;", start)
            path.write_text(content[:start] + marker + "\n" + content[end:])
        # Show VoP results by default, including missing reports. The Postbank
        # option below omits only a complete set of typed positive matches.
        path = source / "src/libs/plugins/backends/aqhbci/applayer/cbox_vopmsg.c"
        replace_once(path,
                     '    DBG_WARN(AQHBCI_LOGDOMAIN, "No msg, no non-matching results, silently accepting.");\n    return 0;',
                     '    if (resultList==NULL) sMsg="Keine Empfängerprüfung verfügbar. Auftrag trotzdem freigeben?";')
        # The user requested removal of the redundant successful-match dialog
        # for Postbank. Keep the protocol, HKVPA and final bank approval intact.
        # Match typed results against every submitted payee, never UI text.
        declaration="int AH_OutboxCBox_LetUserConfirmVopResult(AH_OUTBOX_CBOX *cbox, AH_JOB *workJob, AH_JOB *vppJob, const char *sMsg)"
        replace_once(path, declaration, r'''static int _bankingcraftVopMatchesTransfers(const AH_JOB *job, const AH_VOP_RESULT_LIST *results)
{
  const AB_TRANSACTION_LIST *transfers=AH_Job_GetTransferList(job);
  if (!results || !transfers || AB_Transaction_List_GetCount(transfers)<1 ||
      AH_VopResult_List_GetCount(results)!=AB_Transaction_List_GetCount(transfers) ||
      !AH_VopResult_List_HasOnlyMatches(results)) return 0;
  for (const AB_TRANSACTION *t=AB_Transaction_List_First(transfers);t;t=AB_Transaction_List_Next(t)) {
    const char *iban=AB_Transaction_GetRemoteIban(t);
    int matches=0;
    if (!(iban && *iban)) return 0;
    for (const AH_VOP_RESULT *r=AH_VopResult_List_First(results);r;r=AH_VopResult_List_Next(r)) {
      const char *resultIban=AH_VopResult_GetRemoteIban(r);
      if (resultIban && !strcmp(iban,resultIban)) matches++;
    }
    if (matches!=1) return 0;
  }
  return 1;
}

'''+declaration)
        replace_once(path,
                     "  resultList=vppJob?AH_Job_VPP_GetResultList(vppJob):NULL;",
                     '''  resultList=vppJob?AH_Job_VPP_GetResultList(vppJob):NULL;
  if (AB_Banking_RuntimeConfig_GetIntValue(ab, "bankingcraftSkipMatchingVop", 0) &&
      _bankingcraftVopMatchesTransfers(workJob, resultList)) return 0;''')
        # Use a structured approval callback for push methods (BestSign etc.).
        # Empty text is an acknowledgement to poll HKTAN S, never a TAN or a
        # successful transfer. It also works when HITAN has no challenge text.
        path = source / "src/libs/plugins/backends/aqhbci/applayer/cbox_voptan.c"
        content = path.read_text()
        marker = "  /* BankingCraft: automatic decoupled status polling, no TAN. */"
        if marker not in content:
            start = content.index("int _letUserConfirmApproval(AH_OUTBOX_CBOX *cbox, AH_JOB *workJob, const char *challenge)\n{")
            content = content[:start] + r'''int _letUserConfirmApproval(AH_OUTBOX_CBOX *cbox, AH_JOB *workJob, const char *challenge)
{
  /* BankingCraft: automatic decoupled status polling, no TAN. */
  AB_USER *user=AH_OutboxCBox_GetUser(cbox);
  const AH_TAN_METHOD_LIST *methods=AH_User_GetTanMethodDescriptions(user);
  const char *name="Freigabe in der Banking-App";
  GWEN_DB_NODE *params=GWEN_DB_Group_new("methodParams");
  char acknowledgement[1]={0};
  int rv;
  int attempt=GWEN_DB_GetIntValue(AH_Job_GetArguments(workJob), "bankingcraftApprovalPoll", 0, 0);
  int automatic=0, limit=0, wait=0;
  GWEN_DB_NODE *bpd=GWEN_DB_Group_new("bpd");
  if (AH_Job_SampleBpdVersions("JobTan", user, bpd)>=0) {
    for (GWEN_DB_NODE *version=GWEN_DB_GetFirstGroup(bpd);version;version=GWEN_DB_GetNextGroup(version)) {
      for (GWEN_DB_NODE *m=GWEN_DB_FindFirstGroup(version,"tanMethod");m;m=GWEN_DB_FindNextGroup(m,"tanMethod")) {
        if (1000*atoi(GWEN_DB_GroupName(version))+GWEN_DB_GetIntValue(m,"function",0,0)!=AH_User_GetSelectedTanMethod(user)) continue;
        automatic=!strcasecmp(GWEN_DB_GetCharValue(m,"confirmAllowAutomatic",0,"N"),"J");
        limit=GWEN_DB_GetIntValue(m,"statusQueryMax",0,0);
        wait=GWEN_DB_GetIntValue(m,attempt?"statusWaitNext":"statusWaitFirst",0,-1);
      }
    }
  }
  GWEN_DB_Group_free(bpd);
  if (automatic) {
    if (limit<1 || limit>999 || wait<0 || wait>999 || attempt<0 || attempt>=limit) {
      GWEN_DB_Group_free(params);
      GWEN_Gui_ProgressLog(0, GWEN_LoggerLevel_Error,
        "Automatische Freigabeprüfung beendet: Abfragelimit erreicht oder Bankparameter unvollständig. Bitte Bankstatus vor erneutem Senden prüfen.");
      return GWEN_ERROR_TIMEOUT;
    }
    GWEN_DB_SetIntValue(params, GWEN_DB_FLAGS_OVERWRITE_VARS, "automatic", 1);
    GWEN_DB_SetIntValue(params, GWEN_DB_FLAGS_OVERWRITE_VARS, "wait_seconds", wait);
    GWEN_DB_SetIntValue(params, GWEN_DB_FLAGS_OVERWRITE_VARS, "attempt", attempt+1);
    GWEN_DB_SetIntValue(params, GWEN_DB_FLAGS_OVERWRITE_VARS, "limit", limit);
  }
  for (const AH_TAN_METHOD *m=methods?AH_TanMethod_List_First(methods):NULL;m;m=AH_TanMethod_List_Next(m)) {
    if (AH_TanMethod_GetFunction(m)+1000*AH_TanMethod_GetGvVersion(m)==AH_User_GetSelectedTanMethod(user)) {
      const char *selected=AH_TanMethod_GetMethodName(m);
      if (selected && *selected) name=selected;
      break;
    }
  }
  GWEN_DB_SetIntValue(params, GWEN_DB_FLAGS_OVERWRITE_VARS, "tanMethodId", AB_BANKING_TANMETHOD_DECOUPLED);
  rv=GWEN_Gui_GetPassword(GWEN_GUI_INPUT_FLAGS_TAN | GWEN_GUI_INPUT_FLAGS_SHOW | GWEN_GUI_INPUT_FLAGS_DIRECT,
                         NULL, name,
                         challenge && *challenge?challenge:"Bitte den Auftrag in Ihrer Freigabe-App oder auf Ihrem Freigabegerät prüfen und bestätigen.",
                         acknowledgement, 0, sizeof(acknowledgement), GWEN_Gui_PasswordMethod_Text, params, 0);
  GWEN_DB_Group_free(params);
  if (automatic && rv==0)
    GWEN_DB_SetIntValue(AH_Job_GetArguments(workJob), GWEN_DB_FLAGS_OVERWRITE_VARS, "bankingcraftApprovalPoll", attempt+1);
  return rv;
}
'''
            path.write_text(content)
        # HKTAN S also needs HKVPA when approval has not already been sent in
        # the 3090/3945 exchange. Send it once, not on subsequent 3956 polls.
        replace_once(path,
                     "rv=_sendTanAndReceiveResponseProcS(cbox, dlg, tanJob1, needVpa?vppJob:NULL, workJob);\n    } while (rv==1);",
                     "rv=_sendTanAndReceiveResponseProcS(cbox, dlg, tanJob1, needVpa?vppJob:NULL, workJob);\n      needVpa=0;\n    } while (rv==1);")
        content=path.read_text()
        start=content.index("int _sendTanAndReceiveResponseProcS(AH_OUTBOX_CBOX *cbox, AH_DIALOG *dlg, AH_JOB *tanJob1, AH_JOB *vppJob, AH_JOB *workJob)\n{")
        end=content.index("\nint _sendAndRecvQueue(",start)
        original=content[start:end]
        if "AH_JOB *vpaJob=NULL;" not in original:
            updated=original.replace("  AH_JOB *tanJob2;", "  AH_JOB *tanJob2;\n  AH_JOB *vpaJob=NULL;")
            updated=updated.replace("  jobQueue=_createQueueForStageS(user, tanJob2, workJob);", '''  if (vppJob) {
    vpaJob=_createVpaJob(provider, user, vppJob, workJob);
    if (vpaJob==NULL) { AH_Job_free(tanJob2); return GWEN_ERROR_GENERIC; }
  }
  jobQueue=_createQueueForStageS(user, tanJob2, workJob, vpaJob);
  AH_Job_free(vpaJob);
  if (jobQueue==NULL) { AH_Job_free(tanJob2); return GWEN_ERROR_GENERIC; }
  AH_JobQueue_SubFlags(jobQueue, AH_JOBQUEUE_FLAGS_NEEDTAN);''')
            # The queue owns its reference; release the creator reference on
            # every success, pending and error path.
            updated=updated.replace("    AH_JobQueue_free(jobQueue);\n    return rv;", "    AH_Job_free(tanJob2);\n    AH_JobQueue_free(jobQueue);\n    return rv;")
            updated=updated.replace('    DBG_INFO(AQHBCI_LOGDOMAIN, "here (%d)", rv);\n    return rv;\n  }\n\n  /* dispatch results', '    DBG_INFO(AQHBCI_LOGDOMAIN, "here (%d)", rv);\n    AH_Job_free(tanJob2);\n    AH_JobQueue_free(jobQueue);\n    return rv;\n  }\n\n  /* dispatch results')
            updated=updated.replace("    AH_JobQueue_free(jobQueue);\n    return 1;", "    AH_Job_free(tanJob2);\n    AH_JobQueue_free(jobQueue);\n    return 1;")
            updated=updated.replace("  AH_JobQueue_free(jobQueue);\n  return 0;", "  AH_Job_free(tanJob2);\n  AH_JobQueue_free(jobQueue);\n  return 0;")
            path.write_text(content[:start]+updated+content[end:])
        # Status polling may carry the pending VoP acknowledgement, but never
        # another copy of the payment (unlike the generic TAN-stage-2 queue).
        content=path.read_text()
        old="_createQueueForStageS(AB_USER *user, AH_JOB *tanJobS, AH_JOB *workJob)"
        new="_createQueueForStageS(AB_USER *user, AH_JOB *tanJobS, AH_JOB *workJob, AH_JOB *vpaJob)"
        if old in content:
            if content.count(old)!=2: raise RuntimeError("Unexpected decoupled queue declarations")
            path.write_text(content.replace(old,new))
        replace_once(path, "  AH_Job_Attach(tanJobS);", '''  if (vpaJob) {
    AH_Job_Attach(vpaJob);
    rv=AH_JobQueue_AddJob(jobQueue, vpaJob);
    if (rv!=AH_JobQueueAddResultOk) {
      AH_JobQueue_free(jobQueue);
      return NULL;
    }
  }
  AH_Job_Attach(tanJobS);''')
        replace_once(source / "src/libs/plugins/backends/aqhbci/tan/tanmechanism.c",
                     'tanMechanism= AH_TanMechanism_ChipTanUSB_new(tanMethod, id);',
                     'tanMechanism= AH_TanMechanism_ChipTanOpt_new(tanMethod, AB_BANKING_TANMETHOD_CHIPTAN_OPTIC);')
        # The 6.9.2 loader checks only the legacy HITANS v3 J/N field. v4+
        # uses needTanMediumId with 0=forbidden, 1=optional, 2=required.
        # Keep parsing in AqBanking and expose its existing boolean API correctly.
        replace_once(source / "src/libs/plugins/backends/aqhbci/banking/user.c",
                     '            s=GWEN_DB_GetCharValue(dbT, "tanMediumIdNeeded", 0, NULL);\n'
                     '            if (s && strcasecmp(s, "j")==0)\n'
                     '              AH_TanMethod_SetNeedTanMediumId(tm, 1);',
                     '            s=GWEN_DB_GetCharValue(dbT, "needTanMediumId", 0, NULL);\n'
                     '            if (s) {\n'
                     '              AH_TanMethod_SetNeedTanMediumId(tm, strcmp(s, "2")==0);\n'
                     '            }\n'
                     '            else {\n'
                     '              s=GWEN_DB_GetCharValue(dbT, "tanMediumIdNeeded", 0, NULL);\n'
                     '              if (s && strcasecmp(s, "j")==0)\n'
                     '                AH_TanMethod_SetNeedTanMediumId(tm, 1);\n'
                     '            }')
    if name == "libchipcard":
        # pkg-config 0.29 (Ubuntu 22.04 releases) expands variables in order.
        # Both upstream templates reference datadir before defining it.
        for kind, variable in [("client", "lc_client_xml_dir"), ("server", "lc_server_data_dir")]:
            replace_once(source / f"libchipcard-{kind}.pc.in",
                         f"pkgdatadir=@{variable}@\ndatarootdir=@datarootdir@\ndatadir=@datadir@",
                         f"datarootdir=@datarootdir@\ndatadir=@datadir@\npkgdatadir=@{variable}@")
        # Current Apple C++ compilers reject concatenation without whitespace.
        replace_once(source / "src/tools/cardcommander/cardcommander.cpp",
                     'v"k_CHIPCARD_VERSION_STRING"', 'v" k_CHIPCARD_VERSION_STRING "')
        # Preserve the user's explicit reader selection inside Libchipcard's
        # discovery loop. No card commands are implemented by BankingCraft.
        path = source / "src/lib/client/base/client.c"
        replace_once(path, "          /* card inserted and not used by another application */",
                     '          const char *selected=getenv("BANKINGCRAFT_CARD_READER");\n'
                     '          if (selected && *selected && strcmp(selected,cl->readerStates[i].szReader)) continue;\n'
                     "          /* card inserted and not used by another application */")

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--work", type=Path, default=Path.home() / ".cache/bankingcraft/native")
    parser.add_argument("--jobs", type=int, default=min(os.cpu_count() or 2, 8))
    args = parser.parse_args()
    work = args.work.resolve()
    if any(c.isspace() for c in str(work)):
        raise RuntimeError("Native autotools builds require a work directory without whitespace")
    prefix = work / "install"
    prefix.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env["PKG_CONFIG_PATH"] = str(prefix / "lib/pkgconfig") + os.pathsep + env.get("PKG_CONFIG_PATH", "")
    env["PATH"] = str(prefix / "bin") + os.pathsep + env["PATH"]
    env["LDFLAGS"] = "-L" + (prefix / "lib").as_posix() + " " + env.get("LDFLAGS", "")
    shell = None
    make_tool = "make"
    if os.name == "nt":
        # CreateProcess searches System32 before PATH: bare "bash" starts WSL
        # on hosted runners. Use the MSYS2 installation owning this Python.
        toolchain = Path(sys.executable).resolve().parents[1]
        msys_bin = toolchain.parent / "usr/bin"
        shell = msys_bin / "bash.exe"
        if not shell.is_file():
            raise RuntimeError("Run build-native.py with MSYS2 UCRT64 Python")
        make_tool = str(msys_bin / "make.exe")
        env["SHELL"] = shell.as_posix()
        env["PATH"] = os.pathsep.join([str(prefix / "bin"), str(toolchain / "bin"), str(msys_bin), env["PATH"]])
    if sys.platform == "darwin":
        sdk = subprocess.check_output(["xcrun", "--show-sdk-path"], text=True).strip()
        flags = " -I" + shlex.quote(sdk + "/System/Library/Frameworks/PCSC.framework/Headers")
        env["CFLAGS"] = env.get("CFLAGS", "-O2") + flags
        # Homebrew's keg-only libraries must be discoverable by configure.
        brew = subprocess.check_output(["brew", "--prefix"], text=True).strip()
        env["PATH"] = brew + "/opt/gettext/bin:" + env["PATH"]
        env["CPPFLAGS"] = env.get("CPPFLAGS", "") + " -I" + brew + "/include -I" + brew + "/opt/gettext/include"
        env["LDFLAGS"] = env.get("LDFLAGS", "") + " -L" + brew + "/lib -L" + brew + "/opt/gettext/lib"
        env["PKG_CONFIG_PATH"] += ":" + ":".join(brew + "/opt/" + p + "/lib/pkgconfig" for p in ["openssl@4", "libxml2", "gettext"])
    packages = json.loads((ROOT / "native/dependencies.json").read_text())
    for package in packages:
        stem = package["name"] + "-" + package["version"]
        vendored = ROOT / "native/vendor" / (stem + ".tar.gz")
        archive = vendored if vendored.is_file() else work / (stem + ".tar.gz")
        if not archive.exists():
            print("Downloading", stem, flush=True)
            with urllib.request.urlopen(package["url"], timeout=60) as remote:
                data = remote.read(40 * 1024 * 1024 + 1)
            if len(data) > 40 * 1024 * 1024:
                raise RuntimeError("Source archive exceeds size limit")
            archive.write_bytes(data)
        if hashlib.sha256(archive.read_bytes()).hexdigest() != package["sha256"]:
            raise RuntimeError(f"Source checksum mismatch: {stem}")
        source = work / stem
        if not source.exists():
            with tarfile.open(archive) as tar:
                tar.extractall(work, filter="data")
        patch_source(package["name"], source)
        stamp = source / ".bankingcraft-built-v2"
        recipe = hashlib.sha256(Path(__file__).read_bytes() + (ROOT / "native/dependencies.json").read_bytes() +
            (ROOT / "scripts/patch-instant.py").read_bytes() +
            b"".join(p.read_bytes() for p in sorted((ROOT / "native/aqbanking/instant").glob("*")))).hexdigest()
        if not stamp.exists() or stamp.read_text().strip() != recipe:
            print("Building", stem, flush=True)
            package_env = env.copy()
            make = [make_tool]
            configure = ([str(shell)] if shell else []) + ["./configure", "--prefix=" + prefix.as_posix(), "--disable-static", "--disable-full-doc", "--enable-local-install"]
            if package["name"] == "gwenhywfar":
                # binreloc reads /proc symlinks and is unavailable in native
                # Windows; Gwenhywfar has a separate Win32 path implementation.
                configure += ["--with-guis=none", "--disable-binreloc" if os.name == "nt" else "--enable-binreloc"]
            if package["name"] == "aqbanking":
                configure += ["--with-backends=aqhbci aqnone"]
                # The isolated worker uses the pinned AqHBCI/backend API, which
                # upstream hides on ELF. Export it from our bundled library,
                # as the macOS build already does; never use a system AqBanking.
                package_env["ac_cv_hidden_visibility_attribute"] = "no"
                if os.name == "nt":
                    # Explicit public dllexports disable MinGW's automatic exports.
                    # The worker also needs the internal AB_/AH_ entry points.
                    package_env["LDFLAGS"] += " -Wl,--export-all-symbols"
            with (work / (stem + ".build.log")).open("w") as log:
                try:
                    subprocess.run(configure, cwd=source, env=package_env, stdout=log, stderr=subprocess.STDOUT, check=True)
                    if os.name == "nt":
                        # gettext redirects printf functions on Windows. The
                        # configure scripts clear LIBS, so append the direct
                        # dependency at make time, preserving configured libs.
                        configured = re.search(r"^LIBS[ \t]*=[ \t]*(.*)$", (source / "Makefile").read_text(), re.MULTILINE)
                        if configured is None:
                            raise RuntimeError(f"Cannot determine configured LIBS for {stem}")
                        make += ["LIBS=" + configured.group(1).strip() + " -lintl"]
                    for command in [make + ["clean"], make + ["-j" + str(args.jobs)], make + ["install"]]:
                        subprocess.run(command, cwd=source, env=package_env, stdout=log, stderr=subprocess.STDOUT, check=True)
                except subprocess.CalledProcessError:
                    log.flush()
                    print((work / (stem + ".build.log")).read_text(errors="replace")[-16000:], file=sys.stderr)
                    raise
            stamp.write_text(recipe + "\n")
    print("Building BankingCraft worker", flush=True)
    pkg = ["pkg-config"]
    cflags = shlex.split(subprocess.check_output(pkg + ["--cflags", "aqbanking", "gwenhywfar", "libchipcard-client"], env=env, text=True))
    libs = shlex.split(subprocess.check_output(pkg + ["--libs", "aqbanking", "gwenhywfar", "libchipcard-client"], env=env, text=True))
    if sys.platform.startswith("linux"):
        # pkg-config supplies -L for linking, not a runtime search path.
        libs += ["-Wl,-rpath,$ORIGIN/../lib"]
    executable = prefix / "bin" / ("bankingcraft-aqbanking.exe" if os.name == "nt" or "MINGW" in env.get("MSYSTEM", "") else "bankingcraft-aqbanking")
    subprocess.run([env.get("CC", "cc"), "-std=c11", "-O2", "-Wall", "-Wextra", "-Wno-unused-function", "-Wno-unused-parameter"]
                   + cflags + ["-I" + str(work / "aqbanking-6.9.2/src/libs"), "-I" + str(work / "aqbanking-6.9.2/src/libs/plugins/backends"),
                               "-I" + str(work / "aqbanking-6.9.2/src/libs/plugins/backends/aqhbci"), str(ROOT / "native/aqbanking/main.c")] + libs + ["-o", str(executable)], env=env, check=True)
    (ROOT / ".native").mkdir(exist_ok=True)
    (ROOT / ".native/helper-path").write_text(str(executable) + "\n")
    (ROOT / ".native/install-prefix").write_text(str(prefix) + "\n")
    print(executable)

if __name__ == "__main__":
    main()
