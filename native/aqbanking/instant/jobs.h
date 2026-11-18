/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef BANKINGCRAFT_INSTANT_JOBS_H
#define BANKINGCRAFT_INSTANT_JOBS_H
#include <aqhbci/joblayer/job_l.h>
AH_JOB *BC_InstantTransfer_new(AB_PROVIDER *, AB_USER *, AB_ACCOUNT *);
AH_JOB *BC_InstantStatus_new(AB_PROVIDER *, AB_USER *, AB_ACCOUNT *);
AH_JOB *BC_ScheduledTransfer_new(AB_PROVIDER *, AB_USER *, AB_ACCOUNT *);
#endif
