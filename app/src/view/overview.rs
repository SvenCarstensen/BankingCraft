//! Each overview tile has an independent, stable account-group filter.
use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Scope { #[default] All, Ungrouped, Group(u32) }

impl Scope {
    fn accounts(self, v: &Vault) -> Option<HashSet<u32>> {
        match self {
            Self::All => None,
            Self::Ungrouped => Some(v.accounts.iter().filter(|a| !v.groups.iter().any(|g| g.account_ids.contains(&a.id))).map(|a| a.id).collect()),
            Self::Group(id) => Some(v.groups.iter().find(|g| g.id == id).map(|g| g.account_ids.iter().copied().collect()).unwrap_or_default()),
        }
    }
}

#[derive(Default)]
pub(super) struct Filters {
    choices: Vec<Scope>,
    selected: [Scope; 4], // recent transactions, categories, six months, standing orders
}

pub fn change_group(app: &AppWindow, v: &Vault, vs: &mut ViewState, tile: i32, index: i32) {
    if let (Some(selected), Some(scope)) = (usize::try_from(tile).ok().and_then(|i| vs.overview.selected.get_mut(i)),
        usize::try_from(index).ok().and_then(|i| vs.overview.choices.get(i)).copied()) {
        *selected = scope;
    }
    refresh(app, v, &mut vs.overview);
}

pub(super) fn refresh(app: &AppWindow, v: &Vault, filters: &mut Filters) {
    let st = app.global::<AppState>();
    filters.choices = [Scope::All, Scope::Ungrouped].into_iter().chain(v.groups.iter().map(|g| Scope::Group(g.id))).collect();
    let names = [ss(i18n::tr("Alle Konten")), ss(i18n::tr("Ohne Gruppe"))].into_iter().chain(v.groups.iter().map(|g| ss(&g.name))).collect();
    st.set_overview_group_names(model(names));
    let indices: Vec<i32> = filters.selected.iter_mut().map(|scope| {
        // Renaming/reordering preserves the ID; removing a group resets this tile only.
        if let Some(index) = filters.choices.iter().position(|choice| choice == scope) { index as i32 }
        else { *scope = Scope::All; 0 }
    }).collect();
    st.set_recent_group(indices[0]);
    st.set_category_group(indices[1]);
    st.set_months_group(indices[2]);
    st.set_upcoming_group(indices[3]);

    let today = Local::now().date_naive();
    let start = NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap();
    let accounts = filters.selected[0].accounts(v);
    let mut recent: Vec<usize> = v.transactions.iter().enumerate()
        .filter(|(_, t)| accounts.as_ref().is_none_or(|ids| ids.contains(&t.account_id))).map(|(i, _)| i).collect();
    recent.sort_by(|&a, &b| v.transactions[b].tx.booking_date.cmp(&v.transactions[a].tx.booking_date));
    st.set_recent(model(recent.into_iter().take(6).map(|i| tx_row(v, i, true)).collect()));

    let accounts = filters.selected[1].accounts(v);
    let (_, _, categories) = month_stats_for(v, start, accounts.as_ref());
    st.set_category_stats(model(bar_stats(v, &categories, 6)));
    let accounts = filters.selected[2].accounts(v);
    st.set_months(model(month_series_for(v, start, 6, accounts.as_ref())));

    let accounts = filters.selected[3].accounts(v);
    let mut upcoming: Vec<_> = v.standing.iter().enumerate()
        .filter(|(_, s)| accounts.as_ref().is_none_or(|ids| ids.contains(&s.account_id)))
        .filter_map(|(i, s)| next_execution(&s.order, today).map(|date| (date, i))).collect();
    upcoming.sort_by_key(|(date, _)| *date);
    st.set_upcoming(model(upcoming.into_iter().take(5).map(|(_, i)| standing_row(v, i, today)).collect()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::Model;

    fn fixture() -> (Vault, Vec<u32>, u32, u32) {
        let mut v = Vault::default();
        let ids = v.merge_accounts(1, (0..4).map(|i| banking::Account { account_number: format!("{i}"), product: format!("Account {i}"), ..Default::default() }).collect());
        v.accounts[1].access_id = 2; // A group can span bank connections.
        let a = v.create_group("Personal").unwrap();
        let b = v.create_group("Business").unwrap();
        for &id in &ids[..2] { v.assign_group(id, Some(a)).unwrap(); }
        v.assign_group(ids[2], Some(b)).unwrap();
        let today = Local::now().date_naive();
        let start = today.with_day(1).unwrap();
        for (i, &id) in ids.iter().enumerate() {
            for amount in [10000 * (i as i64 + 1), -1000 * (i as i64 + 1)] {
                let mut tx = crate::business_tests::tx(amount, &start.to_string());
                tx.counterparty_name = format!("Account {i}: {amount}");
                tx.compute_fingerprint();
                v.merge_transactions(id, vec![tx]);
            }
            let mut order = crate::business_tests::standing();
            order.first_date = today + Duration::days(i as i64 + 1);
            order.interval = Interval::Weekly;
            order.every = 1;
            order.last_date = None;
            order.transfer.creditor_name = format!("Order {i}");
            v.standing.push(crate::store::StoredStanding { account_id: id, order });
        }
        // Excluded from financial summaries, still available in the recent list.
        for kind in 0..3 {
            let mut tx = crate::business_tests::tx(-50000 - kind, &start.to_string());
            tx.pending = kind == 0;
            tx.date_unknown = kind == 1;
            if kind == 2 { tx.purpose = "Umbuchung".into(); }
            tx.compute_fingerprint();
            v.merge_transactions(ids[0], vec![tx]);
        }
        let mut old = crate::business_tests::tx(-700, &(start - Duration::days(1)).to_string());
        old.counterparty_name = "Previous month".into();
        old.compute_fingerprint();
        v.merge_transactions(ids[1], vec![old]);
        (v, ids, a, b)
    }

    fn bars(app: &AppWindow) -> Vec<(SharedString, SharedString)> {
        app.global::<AppState>().get_category_stats().iter().map(|s| (s.label, s.value)).collect()
    }
    fn months(app: &AppWindow) -> Vec<(SharedString, SharedString)> {
        app.global::<AppState>().get_months().iter().map(|s| (s.income_text, s.expense_text)).collect()
    }

    #[test]
    fn each_tile_filters_independently_without_changing_global_totals_or_vault_data() {

        let app = AppWindow::new().unwrap();
        let st = app.global::<AppState>();
        let (v, ids, _, _) = fixture();
        let before = serde_json::to_vec(&v).unwrap();
        let mut vs = ViewState::default();
        refresh_all(&app, &v, &mut vs);
        let all_bars = bars(&app);
        let all_months = months(&app);
        let income = st.get_month_income();
        let expense = st.get_month_expense();
        assert_eq!(all_months.last().unwrap(), &(ss("1.000,00 €"), ss("100,00 €")));
        change_group(&app, &v, &mut vs, 0, 2); // Personal, recent only
        assert!(st.get_recent().iter().all(|r| ids[..2].contains(&v.transactions[r.index as usize].account_id)));
        assert_eq!(bars(&app), all_bars);
        assert_eq!(months(&app), all_months);
        assert_eq!(st.get_upcoming().row_count(), 4);
        change_group(&app, &v, &mut vs, 1, 3); // Business categories
        assert_eq!(bars(&app).iter().map(|s| s.1.clone()).collect::<Vec<_>>(), vec![ss("30,00 €")]);
        change_group(&app, &v, &mut vs, 2, 2); // Personal six-month chart
        assert_eq!(months(&app).last().unwrap(), &(ss("300,00 €"), ss("30,00 €")));
        assert_eq!(months(&app)[4].1, "7,00 €");
        change_group(&app, &v, &mut vs, 3, 1); // Ungrouped standing orders
        assert_eq!(st.get_upcoming().iter().map(|s| s.name.to_string()).collect::<Vec<_>>(), vec!["Order 3"]);
        assert_eq!((st.get_recent_group(), st.get_category_group(), st.get_months_group(), st.get_upcoming_group()), (2, 3, 2, 1));
        assert_eq!(st.get_standing().row_count(), 4, "the full standing-order page is unchanged");
        assert_eq!((st.get_month_income(), st.get_month_expense()), (income, expense));
        assert_eq!(serde_json::to_vec(&v).unwrap(), before);
        for tile in 0..4 { change_group(&app, &v, &mut vs, tile, 0); }
        assert_eq!(bars(&app), all_bars);
        assert_eq!(months(&app), all_months);
    }

    #[test]
    fn selection_survives_rename_reorder_translation_and_assignment_but_resets_with_vault() {

        let app = AppWindow::new().unwrap();
        let st = app.global::<AppState>();
        let (mut v, ids, personal, business) = fixture();
        let mut vs = ViewState::default();
        refresh_all(&app, &v, &mut vs);
        for tile in 0..4 { change_group(&app, &v, &mut vs, tile, 2); }
        v.rename_group(personal, "Household").unwrap();
        v.groups.reverse();
        v.assign_group(ids[3], Some(personal)).unwrap();
        i18n::with_language(i18n::Language::English, || refresh_all(&app, &v, &mut vs));
        assert_eq!(st.get_overview_group_names().row_data(0).unwrap(), "All accounts");
        assert_eq!(st.get_overview_group_names().row_data(3).unwrap(), "Household");
        assert_eq!((st.get_recent_group(), st.get_category_group(), st.get_months_group(), st.get_upcoming_group()), (3, 3, 3, 3));
        assert_eq!(months(&app).last().unwrap(), &(ss("700,00 €"), ss("70,00 €")));
        assert_eq!(st.get_upcoming().row_count(), 3);
        for (tile, index) in [(-1, 0), (4, 0), (0, -1), (0, 99)] { change_group(&app, &v, &mut vs, tile, index); }
        assert_eq!(st.get_recent_group(), 3);
        change_group(&app, &v, &mut vs, 1, 2); // retain Business when Personal is removed
        v.delete_group(personal).unwrap();
        refresh_all(&app, &v, &mut vs);
        assert_eq!((st.get_recent_group(), st.get_category_group(), st.get_months_group(), st.get_upcoming_group()), (0, 2, 0, 0));
        assert_eq!(vs.overview.selected[1], Scope::Group(business));
        vs = ViewState::default(); // same reset used when switching vaults
        refresh_all(&app, &Vault::default(), &mut vs);
        assert_eq!(st.get_category_group(), 0);
        assert_eq!(st.get_overview_group_names().row_count(), 2);
        assert_eq!(st.get_recent().row_count(), 0);
        assert_eq!(st.get_upcoming().row_count(), 0);
    }

    #[test]
    fn filtering_precedes_limits_and_empty_groups_do_not_fall_back_to_all_accounts() {

        let app = AppWindow::new().unwrap();
        let st = app.global::<AppState>();
        let (mut v, ids, _, _) = fixture();
        for n in 0..10 {
            let mut tx = crate::business_tests::tx(-100 - n, &Local::now().date_naive().to_string());
            tx.counterparty_name = format!("Business {n}");
            tx.compute_fingerprint();
            v.merge_transactions(ids[2], vec![tx]);
            let mut order = v.standing[2].clone();
            order.order.first_date = Local::now().date_naive();
            v.standing.push(order);
        }
        let empty = v.create_group("Empty").unwrap();
        let mut vs = ViewState::default();
        refresh_all(&app, &v, &mut vs);
        assert_eq!(st.get_recent().row_count(), 6);
        assert_eq!(st.get_upcoming().row_count(), 5);
        change_group(&app, &v, &mut vs, 0, 1);
        change_group(&app, &v, &mut vs, 3, 1);
        assert_eq!(st.get_recent().row_count(), 2);
        assert!(st.get_recent().iter().all(|r| v.transactions[r.index as usize].account_id == ids[3]));
        assert_eq!(st.get_upcoming().row_count(), 1);
        let index = vs.overview.choices.iter().position(|s| *s == Scope::Group(empty)).unwrap() as i32;
        for tile in 0..4 { change_group(&app, &v, &mut vs, tile, index); }
        assert_eq!(st.get_recent().row_count(), 0);
        assert_eq!(st.get_category_stats().row_count(), 0);
        assert_eq!(st.get_upcoming().row_count(), 0);
        assert!(months(&app).iter().all(|(income, expense)| income == "0,00 €" && expense == "0,00 €"));
    }
}
