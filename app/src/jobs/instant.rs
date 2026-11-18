//! Persist every authenticated outcome before a status query can begin.
use super::*;
use banking::{InstantOutcome, InstantPayment};

pub(super) fn submitting_order(ctx: &Ctx, account: u32, end_to_end: &str) -> Result<u32, String> {
    let v = ctx.vault.lock().unwrap();
    let mut orders = v.payment_orders.iter().filter(|o| o.account_id == account && o.transfer.instant
        && o.status == OrderStatus::Submitting && !end_to_end.is_empty() && o.transfer.end_to_end_id == end_to_end);
    let id = orders.next().ok_or("Echtzeitauftrag nicht eindeutig zugeordnet.")?.id;
    if orders.next().is_some() { return Err("Echtzeitauftrag nicht eindeutig zugeordnet.".into()); }
    Ok(id)
}
pub(super) fn persist(ctx: &Ctx, id: u32, result: &InstantPayment) -> Result<(), String> {
    let mut v = ctx.vault.lock().unwrap();
    let order = v.payment_orders.iter_mut().find(|o| o.id == id).ok_or("Auftrag nicht gefunden")?;
    if !order.transfer.instant || order.transfer.debtor_iban != result.debtor_iban
        || !matches!(order.status, OrderStatus::Submitting | OrderStatus::NeedsReview)
        || order.instant_payment.as_ref().is_some_and(|p| !p.reference.is_empty() && p.reference != result.reference) {
        return Err("Bankrückmeldung passt nicht zum Echtzeitauftrag.".into());
    }
    // Retain the newest result in memory even if saving fails. The on-disk
    // Submitting/NeedsReview marker always prevents re-sending the payment.
    order.instant_payment = Some(result.clone());
    order.message = i18n::tr(result.message()).into();
    ctx.file.lock().unwrap().as_ref().ok_or("Tresor ist gesperrt")?.save(&v)
}
pub fn check_instant(ctx: &Ctx, id: u32) -> JobResult {
    let order = ctx.vault.lock().unwrap().payment_orders.iter().find(|o| o.id == id).cloned()
        .ok_or("Auftrag nicht gefunden")?;
    let previous = order.instant_status_query().ok_or("Für diesen Auftrag ist keine Statusabfrage möglich.")?;
    if !matches!(order.status, OrderStatus::Submitting | OrderStatus::NeedsReview) { return Err("Auftrag ist bereits abgeschlossen.".into()); }
    let (account, access) = account_and_access(ctx, order.account_id)?;
    let bridge = ctx.bridge.clone();
    let mut client = make_client(ctx, &access, &bridge)?;
    client.observe_instant(move |result| persist(ctx, id, result).map_err(Error::Other));
    let result = client.instant_status(&account, &previous);
    let result = conclude(ctx, access.id, client, result);
    finish_check(ctx, id, result)
}
fn finish_check(ctx: &Ctx, id: u32, result: JobResult) -> JobResult {
    {
        let mut v = ctx.vault.lock().unwrap();
        let order = v.payment_orders.iter_mut().find(|o| o.id == id).ok_or("Auftrag nicht gefunden")?;
        let executed = result.is_ok() && order.instant_payment.as_ref().is_some_and(|p| p.outcome == InstantOutcome::Executed);
        order.status = if executed { OrderStatus::Sent } else { OrderStatus::NeedsReview };
        order.message = match &result { Ok(message) | Err(message) => message.clone() };
    }
    ctx.save()?;
    result
}
