"""Reviewed extensions to pinned AqBanking 6.9.2; see docs/instant-payments.md."""
from pathlib import Path
import xml.etree.ElementTree as ET

ROOT=Path(__file__).resolve().parents[1]

def patch(source, replace):
    base=source/'src/libs/plugins/backends/aqhbci'
    owned=ROOT/'native/aqbanking/instant'
    for name in ['jobs.c','jobs.h']:
        (base/'ajobs'/('bankingcraft_instant_'+name)).write_bytes((owned/name).read_bytes())
    replace(base/'ajobs/jobsepaxfersingle.c', '#include "jobsepaxfersingle_l.h"',
            '#include "jobsepaxfersingle_l.h"\n#include "bankingcraft_instant_jobs.h"')
    replace(base/'ajobs/jobsepaxfersingle.c', '#include "jobtransferbase_l.h"',
            '#include "jobtransferbase_l.h"\n#include "bankingcraft_instant_jobs.c"')
    replace(base/'banking/provider_job.c', '#include "provider_job.h"',
            '#include "provider_job.h"\n#include "aqhbci/ajobs/bankingcraft_instant_jobs.h"')
    replace(base/'banking/provider_job.c', '  case AB_Transaction_CommandSepaTransfer:\n    if',
            '''  case AB_Transaction_CommandSepaInstantTransfer:
    mj=BC_InstantTransfer_new(pro, mu, ma);
    if (!mj) return GWEN_ERROR_NOT_AVAILABLE;
    break;
  case AB_Transaction_CommandGetSepaInstantStatus:
    mj=BC_InstantStatus_new(pro, mu, ma);
    if (!mj) return GWEN_ERROR_NOT_AVAILABLE;
    break;
  case AB_Transaction_CommandSepaTransfer:
    if''')
    replace(base/'banking/provider_job.c', '  case AB_Transaction_CommandSepaInstantTransfer:',
            '''  case AB_Transaction_CommandSepaCreateDatedTransfer:
    mj=BC_ScheduledTransfer_new(pro, mu, ma);
    if (!mj) return GWEN_ERROR_NOT_AVAILABLE;
    break;
  case AB_Transaction_CommandSepaInstantTransfer:''')
    replace(base/'banking/provider_accspec.c', '    AB_Transaction_CommandSepaTransfer,',
            '    AB_Transaction_CommandSepaTransfer,\n    AB_Transaction_CommandSepaInstantTransfer,\n    AB_Transaction_CommandGetSepaInstantStatus,')
    replace(base/'banking/provider_accspec.c', '    AB_Transaction_CommandGetSepaInstantStatus,',
            '    AB_Transaction_CommandGetSepaInstantStatus,\n    AB_Transaction_CommandSepaCreateDatedTransfer,')
    types=source/'src/libs/aqbanking/types'
    replace(types/'transaction.xml', '        <item name="sepaInternalTransfer"/>',
            '        <item name="sepaInternalTransfer"/>\n        <item name="sepaInstantTransfer" value="1550"/>\n        <item name="getSepaInstantStatus" value="299"/>')
    # Update the shipped generated files as well (release tarballs need not
    # have the maintainer's typemaker tool enabled). Existing values are stable.
    replace(types/'transaction.h','  AB_Transaction_CommandSepaTransfer = 1536,',
            '  AB_Transaction_CommandSepaInstantTransfer = 1550,\n  AB_Transaction_CommandGetSepaInstantStatus = 299,\n  AB_Transaction_CommandSepaTransfer = 1536,')
    replace(types/'transaction.c','    else if (strcasecmp(p_s, "sepaTransfer")==0)',
            '    else if (strcasecmp(p_s, "sepaInstantTransfer")==0)\n      return AB_Transaction_CommandSepaInstantTransfer;\n    else if (strcasecmp(p_s, "getSepaInstantStatus")==0)\n      return AB_Transaction_CommandGetSepaInstantStatus;\n    else if (strcasecmp(p_s, "sepaTransfer")==0)')
    replace(types/'transaction.c','    case AB_Transaction_CommandSepaTransfer: return "sepaTransfer";',
            '    case AB_Transaction_CommandSepaInstantTransfer: return "sepaInstantTransfer";\n    case AB_Transaction_CommandGetSepaInstantStatus: return "getSepaInstantStatus";\n    case AB_Transaction_CommandSepaTransfer: return "sepaTransfer";')
    # Merge into an already-built schema input; no generated Makefile patch.
    xml=base/'ajobs/jobsepaxfersingle.xml'
    original=xml.read_text()
    for group in ['SEGs','JOBs']:
        begin=f'<!-- BankingCraft instant {group} -->'
        end=f'<!-- End BankingCraft instant {group} -->'
        if begin in original:
            start=original.index(begin); stop=original.index(end,start)+len(end)
            original=original[:start].rstrip()+"\n"+original[stop:].lstrip("\n")
        nodes=ET.fromstring((owned/'segments.xml').read_text()).find(group)
        extra='\n'.join(ET.tostring(n,encoding='unicode') for n in nodes)
        original=original.replace(f'</{group}>',f'{begin}\n{extra}\n{end}\n</{group}>')
    if xml.read_text()!=original: xml.write_text(original)
    # The exporter already copies LocalInstrumentSEPACode from its profile to
    # every payment group. CORE is its legacy default: emit INST only explicitly.
    for profile in ['pain_001_001_03','pain_001_003_03','pain_001_001_09']:
        path=source/'src/libs/plugins/imexporters/xml/data'/(profile+'.xml')
        content=path.read_text()
        marker='<!-- BankingCraft SCT Inst -->'
        if marker not in content:
            import re
            pattern=r'(<XmlSetCharValue\s+path="PmtTpInf/SvcLvl/Cd"\s+value="SEPA"\s*/>)'
            content,count=re.subn(pattern,r'''\1
            <!-- BankingCraft SCT Inst -->
            <DbIfCharDataMatches name="localInstrumentSEPACode" pattern="INST">
              <XmlSetCharValue path="PmtTpInf/LclInstrm/Cd" value="INST" />
            </DbIfCharDataMatches>''',content)
            if count!=1: raise RuntimeError(f'Unexpected credit-transfer schema: {profile}')
            path.write_text(content)
