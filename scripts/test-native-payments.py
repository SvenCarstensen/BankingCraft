#!/usr/bin/env python3
"""Offline FinTS payment protocol regression tests, ASan/UBSan, no bank IO."""
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET
from datetime import date, timedelta
ROOT=Path(__file__).resolve().parents[1]
prefix=Path((ROOT/'.native/install-prefix').read_text().strip())
source=prefix.parent/'aqbanking-6.9.2/src/libs'
env=os.environ.copy();env['PKG_CONFIG_PATH']=str(prefix/'lib/pkgconfig')
flags=shlex.split(subprocess.check_output(['pkg-config','--cflags','--libs','aqbanking','gwenhywfar','libchipcard-client'],env=env,text=True))
if sys.platform.startswith('linux'): flags+=['-Wl,-rpath,$ORIGIN/../lib']
binary=prefix/'bin/bankingcraft-payment-tests'
subprocess.run([env.get('CC','cc'),'-std=c11','-g','-fsanitize=address,undefined','-I'+str(source),'-I'+str(source/'plugins/backends'),'-I'+str(source/'plugins/backends/aqhbci'),str(ROOT/'native/aqbanking/tests_payments.c'),*flags,'-o',str(binary)],check=True)
with tempfile.TemporaryDirectory(prefix='bankingcraft-payment-tests-') as directory:
    root=Path(directory);blocker=root/'blocked';blocker.touch();env['HOME']=directory
    result=subprocess.run([str(binary),str(blocker),directory],env=env,capture_output=True,text=True,timeout=60)
    if result.returncode: raise RuntimeError(result.stderr[-12000:])
    for code in ['HKIPZ','HKCSE','HKCCS']:
        for profile in ['pain.001.001.09','pain.001.001.03','pain.001.003.03']:
            doc=ET.parse(root/f'{code}-{profile}.xml');ns={'p':f'urn:iso:std:iso:20022:tech:xsd:{profile}'}
            get=lambda path:doc.findtext('.//p:'+path.replace('/','/p:'),namespaces=ns)
            assert get('NbOfTxs')=='1'
            assert get('PmtInf/PmtTpInf/SvcLvl/Cd')=='SEPA'
            assert get('PmtInf/PmtTpInf/LclInstrm/Cd')==('INST' if code=='HKIPZ' else None)
            execution=get('PmtInf/ReqdExctnDt/Dt') if profile.endswith('.09') else get('PmtInf/ReqdExctnDt')
            if code=='HKIPZ': assert execution=='1999-01-01'
            if code=='HKCSE': assert execution==(date.today()+timedelta(days=5)).isoformat()
            assert get('PmtInf/DbtrAcct/Id/IBAN')=='DE89370400440532013000'
            assert get('PmtInf/CdtTrfTxInf/CdtrAcct/Id/IBAN')=='DE02120300000000202051'
            assert get('PmtInf/CdtTrfTxInf/Amt/InstdAmt')=='1.23'
            assert get('PmtInf/CdtTrfTxInf/PmtId/EndToEndId')=='OFFLINE-ONLY-123'
    print(result.stdout.strip())
    print('Validated 9 payment XML variants (instant, scheduled, standard; 3 bank formats).')
