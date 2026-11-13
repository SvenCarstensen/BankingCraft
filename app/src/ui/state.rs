//! UI view models. Banking data remains owned by the vault.
use super::{Color, Image, ModelRc, SharedString};
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AccountDropTarget {
    pub id: i32,
    pub after: bool,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AccountRow {
    pub id: i32,
    pub name: SharedString,
    pub bank: SharedString,
    pub iban: SharedString,
    pub owner: SharedString,
    pub balance: SharedString,
    pub available: SharedString,
    pub negative: bool,
    pub last_sync: SharedString,
    pub kind: SharedString,
    pub color: Color,
    pub group_index: i32,
    pub group_name: SharedString,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GroupBalanceRow {
    pub amount: SharedString,
    pub negative: bool,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AccountGroupRow {
    pub id: i32,
    pub name: SharedString,
    pub accounts: ModelRc<AccountRow>,
    pub balances: ModelRc<GroupBalanceRow>,
    pub balance_note: SharedString,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TxRow {
    pub is_header: bool,
    pub date_label: SharedString,
    pub day_balance: SharedString,
    pub index: i32,
    pub name: SharedString,
    pub purpose: SharedString,
    pub amount: SharedString,
    pub negative: bool,
    pub pending: bool,
    pub category: SharedString,
    pub cat_color: Color,
    pub initial: SharedString,
    pub account: SharedString,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TxDetail {
    pub valid: bool,
    pub index: i32,
    pub name: SharedString,
    pub iban: SharedString,
    pub bic: SharedString,
    pub amount: SharedString,
    pub negative: bool,
    pub booking_date: SharedString,
    pub value_date: SharedString,
    pub purpose: SharedString,
    pub posting_text: SharedString,
    pub eref: SharedString,
    pub mref: SharedString,
    pub creditor_id: SharedString,
    pub category: SharedString,
    pub account: SharedString,
    pub pending: bool,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BarStat {
    pub label: SharedString,
    pub value: SharedString,
    pub ratio: f32,
    pub color: Color,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MonthStat {
    pub label: SharedString,
    pub income: f32,
    pub expense: f32,
    pub income_text: SharedString,
    pub expense_text: SharedString,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StandingRow {
    pub index: i32,
    pub name: SharedString,
    pub iban: SharedString,
    pub amount: SharedString,
    pub purpose: SharedString,
    pub schedule: SharedString,
    pub next: SharedString,
    pub account: SharedString,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AccessRow {
    pub id: i32,
    pub name: SharedString,
    pub kind: SharedString,
    pub detail: SharedString,
    pub tan_method: SharedString,
    pub accounts: i32,
    pub demo: bool,
    pub pin_saved: bool,
    pub card_based: bool,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BankRow {
    pub index: i32,
    pub name: SharedString,
    pub detail: SharedString,
    pub endpoint: SharedString,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChoiceRow {
    pub key: SharedString,
    pub title: SharedString,
    pub subtitle: SharedString,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Toast {
    pub visible: bool,
    pub text: SharedString,
    pub error: bool,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PaymentOrderRow {
    pub id: i32,
    pub name: SharedString,
    pub iban: SharedString,
    pub amount: SharedString,
    pub purpose: SharedString,
    pub account: SharedString,
    pub execution: SharedString,
    pub status: SharedString,
    pub detail: SharedString,
    pub needs_review: bool,
    pub queued: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Data {
    pub unlocked: bool,
    pub first_run: bool,
    pub show_vault_manager: bool,
    pub recent_vaults: ModelRc<SharedString>,
    pub selected_vault: i32,
    pub selected_vault_name: SharedString,
    pub selected_vault_path: SharedString,
    pub active_vault_name: SharedString,
    pub vault_selection_revision: i32,
    pub unlock_error: SharedString,
    pub page: i32,
    pub busy: bool,
    pub busy_text: SharedString,
    pub toast: Toast,
    pub last_sync: SharedString,
    pub total_balance: SharedString,
    pub total_negative: bool,
    pub month_label: SharedString,
    pub month_income: SharedString,
    pub month_expense: SharedString,
    pub accounts: ModelRc<AccountRow>,
    pub account_groups: ModelRc<AccountGroupRow>,
    pub group_names: ModelRc<SharedString>,
    pub new_group_name: SharedString,
    pub account_names: ModelRc<SharedString>,
    pub recent: ModelRc<TxRow>,
    pub category_stats: ModelRc<BarStat>,
    pub months: ModelRc<MonthStat>,
    pub upcoming: ModelRc<StandingRow>,
    pub overview_group_names: ModelRc<SharedString>,
    pub recent_group: i32,
    pub category_group: i32,
    pub months_group: i32,
    pub upcoming_group: i32,
    pub show_account_name: bool,
    pub editing_account_id: i32,
    pub account_name_draft: SharedString,
    pub account_bank_name: SharedString,
    pub account_name_context: SharedString,
    pub tx_rows: ModelRc<TxRow>,
    pub tx_account: i32,
    pub tx_search: SharedString,
    pub tx_category: SharedString,
    pub tx_period: i32,
    pub tx_summary: SharedString,
    pub tx_detail: TxDetail,
    pub categories: ModelRc<SharedString>,
    pub category_labels: ModelRc<SharedString>,
    pub category_colors: ModelRc<Color>,
    pub sepa_accounts: ModelRc<SharedString>,
    pub tf_account: i32,
    pub payment_orders: ModelRc<PaymentOrderRow>,
    pub queued_order_count: i32,
    pub recipient_names: ModelRc<SharedString>,
    pub tf_recipient: i32,
    pub tf_save_recipient: bool,
    pub tf_name: SharedString,
    pub tf_iban: SharedString,
    pub tf_bic: SharedString,
    pub tf_amount: SharedString,
    pub tf_purpose: SharedString,
    pub tf_mode: i32,
    pub tf_date: SharedString,
    pub tf_iban_hint: SharedString,
    pub tf_iban_error: bool,
    pub so_interval: i32,
    pub so_first: SharedString,
    pub so_day: SharedString,
    pub so_last: SharedString,
    pub standing: ModelRc<StandingRow>,
    pub analysis_categories: ModelRc<BarStat>,
    pub analysis_income: ModelRc<BarStat>,
    pub analysis_months: ModelRc<MonthStat>,
    pub analysis_label: SharedString,
    pub analysis_offset: i32,
    pub accesses: ModelRc<AccessRow>,
    pub af_open: bool,
    pub af_kind: i32,
    pub af_statement_format: i32,
    pub af_name: SharedString,
    pub af_blz: SharedString,
    pub af_user: SharedString,
    pub af_customer: SharedString,
    pub af_url: SharedString,
    pub af_reader: i32,
    pub af_record: SharedString,
    pub bank_query: SharedString,
    pub bank_results: ModelRc<BankRow>,
    pub bank_picked: SharedString,
    pub reader_names: ModelRc<SharedString>,
    pub reader_report: ModelRc<SharedString>,
    pub cyberjack_found: bool,
    pub tan_reader: i32,
    pub theme_mode: i32,
    pub product_id: SharedString,
    pub product_version: SharedString,
    pub data_path: SharedString,
    pub old_password: SharedString,
    pub new_password: SharedString,
    pub show_pin: bool,
    pub pin_title: SharedString,
    pub pin_value: SharedString,
    pub pin_heading: SharedString,
    pub pin_masked: bool,
    pub pin_offer_save: bool,
    pub pin_save: bool,
    pub paste_note: SharedString,
    pub show_pinpad: bool,
    pub pinpad_text: SharedString,
    pub show_tan: bool,
    pub tan_title: SharedString,
    pub tan_text: SharedString,
    pub tan_kind: i32,
    pub tan_value: SharedString,
    pub tan_image: Image,
    pub tan_usb_possible: bool,
    pub flick0: bool,
    pub flick1: bool,
    pub flick2: bool,
    pub flick3: bool,
    pub flick4: bool,
    pub flicker_width: f32,
    pub flicker_hz: i32,
    pub show_choice: bool,
    pub choice_title: SharedString,
    pub choices: ModelRc<ChoiceRow>,
    pub show_confirm: bool,
    pub confirm_title: SharedString,
    pub confirm_text: SharedString,
    pub confirm_danger: bool,
    pub confirm_ok_text: SharedString,
    pub show_vop: bool,
    pub vop_status: i32,
    pub vop_name: SharedString,
    pub vop_iban: SharedString,
    pub vop_suggested_name: SharedString,
    pub vop_reason: SharedString,
    pub vop_notice: SharedString,
    pub vop_executed: bool,
    pub language_mode: i32,
}
impl Default for Data {
    fn default() -> Self {
        Self {
            unlocked: Default::default(),
            first_run: Default::default(),
            show_vault_manager: Default::default(),
            recent_vaults: Default::default(),
            selected_vault: -1,
            selected_vault_name: Default::default(),
            selected_vault_path: Default::default(),
            active_vault_name: Default::default(),
            vault_selection_revision: Default::default(),
            unlock_error: Default::default(),
            page: Default::default(),
            busy: Default::default(),
            busy_text: Default::default(),
            toast: Default::default(),
            last_sync: Default::default(),
            total_balance: Default::default(),
            total_negative: Default::default(),
            month_label: Default::default(),
            month_income: Default::default(),
            month_expense: Default::default(),
            accounts: Default::default(),
            account_groups: Default::default(),
            group_names: Default::default(),
            new_group_name: Default::default(),
            account_names: Default::default(),
            recent: Default::default(),
            category_stats: Default::default(),
            months: Default::default(),
            upcoming: Default::default(),
            overview_group_names: Default::default(),
            recent_group: Default::default(),
            category_group: Default::default(),
            months_group: Default::default(),
            upcoming_group: Default::default(),
            show_account_name: Default::default(),
            editing_account_id: Default::default(),
            account_name_draft: Default::default(),
            account_bank_name: Default::default(),
            account_name_context: Default::default(),
            tx_rows: Default::default(),
            tx_account: Default::default(),
            tx_search: Default::default(),
            tx_category: Default::default(),
            tx_period: 2,
            tx_summary: Default::default(),
            tx_detail: Default::default(),
            categories: Default::default(),
            category_labels: Default::default(),
            category_colors: Default::default(),
            sepa_accounts: Default::default(),
            tf_account: Default::default(),
            payment_orders: Default::default(),
            queued_order_count: Default::default(),
            recipient_names: Default::default(),
            tf_recipient: Default::default(),
            tf_save_recipient: Default::default(),
            tf_name: Default::default(),
            tf_iban: Default::default(),
            tf_bic: Default::default(),
            tf_amount: Default::default(),
            tf_purpose: Default::default(),
            tf_mode: Default::default(),
            tf_date: Default::default(),
            tf_iban_hint: Default::default(),
            tf_iban_error: Default::default(),
            so_interval: Default::default(),
            so_first: Default::default(),
            so_day: "1".into(),
            so_last: Default::default(),
            standing: Default::default(),
            analysis_categories: Default::default(),
            analysis_income: Default::default(),
            analysis_months: Default::default(),
            analysis_label: Default::default(),
            analysis_offset: Default::default(),
            accesses: Default::default(),
            af_open: Default::default(),
            af_kind: Default::default(),
            af_statement_format: Default::default(),
            af_name: Default::default(),
            af_blz: Default::default(),
            af_user: Default::default(),
            af_customer: Default::default(),
            af_url: Default::default(),
            af_reader: Default::default(),
            af_record: "1".into(),
            bank_query: Default::default(),
            bank_results: Default::default(),
            bank_picked: Default::default(),
            reader_names: Default::default(),
            reader_report: Default::default(),
            cyberjack_found: Default::default(),
            tan_reader: Default::default(),
            theme_mode: Default::default(),
            product_id: Default::default(),
            product_version: Default::default(),
            data_path: Default::default(),
            old_password: Default::default(),
            new_password: Default::default(),
            show_pin: Default::default(),
            pin_title: Default::default(),
            pin_value: Default::default(),
            pin_heading: String::new(),
            pin_masked: true,
            pin_offer_save: Default::default(),
            pin_save: Default::default(),
            paste_note: Default::default(),
            show_pinpad: Default::default(),
            pinpad_text: Default::default(),
            show_tan: Default::default(),
            tan_title: Default::default(),
            tan_text: Default::default(),
            tan_kind: Default::default(),
            tan_value: Default::default(),
            tan_image: Default::default(),
            tan_usb_possible: Default::default(),
            flick0: Default::default(),
            flick1: Default::default(),
            flick2: Default::default(),
            flick3: Default::default(),
            flick4: Default::default(),
            flicker_width: 260.,
            flicker_hz: 15,
            show_choice: Default::default(),
            choice_title: Default::default(),
            choices: Default::default(),
            show_confirm: Default::default(),
            confirm_title: Default::default(),
            confirm_text: Default::default(),
            confirm_danger: Default::default(),
            confirm_ok_text: Default::default(),
            show_vop: Default::default(),
            vop_status: Default::default(),
            vop_name: Default::default(),
            vop_iban: Default::default(),
            vop_suggested_name: Default::default(),
            vop_reason: Default::default(),
            vop_notice: Default::default(),
            vop_executed: Default::default(),
            language_mode: 0,
        }
    }
}

type Callback<F> = Option<std::rc::Rc<F>>;
#[derive(Default)]
pub struct Callbacks {
    vop_accept: Callback<dyn Fn()>,
    vop_cancel: Callback<dyn Fn()>,
    vop_copy_name: Callback<dyn Fn()>,
    unlock: Callback<dyn Fn(SharedString)>,
    create_vault: Callback<dyn Fn(SharedString, SharedString)>,
    manage_vaults: Callback<dyn Fn()>,
    cancel_vault_selection: Callback<dyn Fn()>,
    choose_vault: Callback<dyn Fn(i32)>,
    open_vault_file: Callback<dyn Fn()>,
    new_vault_file: Callback<dyn Fn()>,
    navigate: Callback<dyn Fn(i32)>,
    refresh_all: Callback<dyn Fn()>,
    refresh_transactions: Callback<dyn Fn()>,
    refresh_group: Callback<dyn Fn(i32)>,
    overview_group_changed: Callback<dyn Fn(i32, i32)>,
    open_account: Callback<dyn Fn(i32)>,
    edit_account_name: Callback<dyn Fn(i32)>,
    save_account_name: Callback<dyn Fn()>,
    account_row_layout: Callback<dyn Fn(i32, f32, f32)>,
    account_drop_target: Callback<dyn Fn(f32) -> AccountDropTarget>,
    create_group: Callback<dyn Fn(SharedString)>,
    rename_group: Callback<dyn Fn(i32, SharedString)>,
    delete_group: Callback<dyn Fn(i32)>,
    assign_group: Callback<dyn Fn(i32, i32)>,
    move_account: Callback<dyn Fn(i32, i32, bool) -> bool>,
    tx_filter_changed: Callback<dyn Fn()>,
    tx_select: Callback<dyn Fn(i32)>,
    set_category: Callback<dyn Fn(i32, SharedString)>,
    export_csv: Callback<dyn Fn()>,
    export_camt: Callback<dyn Fn()>,
    check_iban: Callback<dyn Fn(SharedString)>,
    send_transfer: Callback<dyn Fn()>,
    save_transfer: Callback<dyn Fn()>,
    delete_payment_order: Callback<dyn Fn(i32)>,
    payment_account_changed: Callback<dyn Fn()>,
    select_recipient: Callback<dyn Fn(i32)>,
    delete_recipient: Callback<dyn Fn()>,
    send_standing: Callback<dyn Fn()>,
    fetch_standing: Callback<dyn Fn()>,
    delete_standing: Callback<dyn Fn(i32)>,
    new_access: Callback<dyn Fn()>,
    save_access: Callback<dyn Fn()>,
    delete_access: Callback<dyn Fn(i32)>,
    sync_access: Callback<dyn Fn(i32)>,
    reset_tan: Callback<dyn Fn(i32)>,
    forget_pin: Callback<dyn Fn(i32)>,
    read_card: Callback<dyn Fn()>,
    bank_search: Callback<dyn Fn(SharedString)>,
    bank_pick: Callback<dyn Fn(i32)>,
    reader_refresh: Callback<dyn Fn()>,
    reader_diagnose: Callback<dyn Fn()>,
    save_settings: Callback<dyn Fn()>,
    change_theme: Callback<dyn Fn(i32)>,
    change_language: Callback<dyn Fn(i32)>,
    load_demo: Callback<dyn Fn()>,
    change_password: Callback<dyn Fn()>,
    analysis_shift: Callback<dyn Fn(i32)>,
    pin_ok: Callback<dyn Fn()>,
    pin_paste: Callback<dyn Fn()>,
    tan_paste: Callback<dyn Fn()>,
    pin_cancel: Callback<dyn Fn()>,
    tan_ok: Callback<dyn Fn()>,
    tan_cancel: Callback<dyn Fn()>,
    tan_confirmed: Callback<dyn Fn()>,
    tan_usb: Callback<dyn Fn()>,
    flicker_speed: Callback<dyn Fn(i32)>,
    choice_picked: Callback<dyn Fn(SharedString)>,
    choice_cancel: Callback<dyn Fn()>,
    confirm_ok: Callback<dyn Fn()>,
    confirm_cancel: Callback<dyn Fn()>,
    toast_close: Callback<dyn Fn()>,
}
#[derive(Clone, Default)]
pub struct AppState {
    data: std::rc::Rc<std::cell::RefCell<Data>>,
    callbacks: std::rc::Rc<std::cell::RefCell<Callbacks>>,
}
#[allow(dead_code)]
impl AppState {
    pub fn snapshot(&self) -> Data {
        self.data.borrow().clone()
    }
    pub fn patch(&self, before: &Data, after: &Data) {
        let mut data = self.data.borrow_mut();
        if before.unlocked != after.unlocked {
            data.unlocked = after.unlocked;
        }
        if before.first_run != after.first_run {
            data.first_run = after.first_run;
        }
        if before.show_vault_manager != after.show_vault_manager {
            data.show_vault_manager = after.show_vault_manager;
        }
        if before.recent_vaults != after.recent_vaults {
            data.recent_vaults = after.recent_vaults.clone();
        }
        if before.selected_vault != after.selected_vault {
            data.selected_vault = after.selected_vault;
        }
        if before.selected_vault_name != after.selected_vault_name {
            data.selected_vault_name = after.selected_vault_name.clone();
        }
        if before.selected_vault_path != after.selected_vault_path {
            data.selected_vault_path = after.selected_vault_path.clone();
        }
        if before.active_vault_name != after.active_vault_name {
            data.active_vault_name = after.active_vault_name.clone();
        }
        if before.vault_selection_revision != after.vault_selection_revision {
            data.vault_selection_revision = after.vault_selection_revision;
        }
        if before.unlock_error != after.unlock_error {
            data.unlock_error = after.unlock_error.clone();
        }
        if before.page != after.page {
            data.page = after.page;
        }
        if before.busy != after.busy {
            data.busy = after.busy;
        }
        if before.busy_text != after.busy_text {
            data.busy_text = after.busy_text.clone();
        }
        if before.toast != after.toast {
            data.toast = after.toast.clone();
        }
        if before.last_sync != after.last_sync {
            data.last_sync = after.last_sync.clone();
        }
        if before.total_balance != after.total_balance {
            data.total_balance = after.total_balance.clone();
        }
        if before.total_negative != after.total_negative {
            data.total_negative = after.total_negative;
        }
        if before.month_label != after.month_label {
            data.month_label = after.month_label.clone();
        }
        if before.month_income != after.month_income {
            data.month_income = after.month_income.clone();
        }
        if before.month_expense != after.month_expense {
            data.month_expense = after.month_expense.clone();
        }
        if before.accounts != after.accounts {
            data.accounts = after.accounts.clone();
        }
        if before.account_groups != after.account_groups {
            data.account_groups = after.account_groups.clone();
        }
        if before.group_names != after.group_names {
            data.group_names = after.group_names.clone();
        }
        if before.new_group_name != after.new_group_name {
            data.new_group_name = after.new_group_name.clone();
        }
        if before.account_names != after.account_names {
            data.account_names = after.account_names.clone();
        }
        if before.recent != after.recent {
            data.recent = after.recent.clone();
        }
        if before.category_stats != after.category_stats {
            data.category_stats = after.category_stats.clone();
        }
        if before.months != after.months {
            data.months = after.months.clone();
        }
        if before.upcoming != after.upcoming {
            data.upcoming = after.upcoming.clone();
        }
        if before.overview_group_names != after.overview_group_names {
            data.overview_group_names = after.overview_group_names.clone();
        }
        if before.recent_group != after.recent_group {
            data.recent_group = after.recent_group;
        }
        if before.category_group != after.category_group {
            data.category_group = after.category_group;
        }
        if before.months_group != after.months_group {
            data.months_group = after.months_group;
        }
        if before.upcoming_group != after.upcoming_group {
            data.upcoming_group = after.upcoming_group;
        }
        if before.show_account_name != after.show_account_name {
            data.show_account_name = after.show_account_name;
        }
        if before.editing_account_id != after.editing_account_id {
            data.editing_account_id = after.editing_account_id;
        }
        if before.account_name_draft != after.account_name_draft {
            data.account_name_draft = after.account_name_draft.clone();
        }
        if before.account_bank_name != after.account_bank_name {
            data.account_bank_name = after.account_bank_name.clone();
        }
        if before.account_name_context != after.account_name_context {
            data.account_name_context = after.account_name_context.clone();
        }
        if before.tx_rows != after.tx_rows {
            data.tx_rows = after.tx_rows.clone();
        }
        if before.tx_account != after.tx_account {
            data.tx_account = after.tx_account;
        }
        if before.tx_search != after.tx_search {
            data.tx_search = after.tx_search.clone();
        }
        if before.tx_category != after.tx_category {
            data.tx_category = after.tx_category.clone();
        }
        if before.tx_period != after.tx_period {
            data.tx_period = after.tx_period;
        }
        if before.tx_summary != after.tx_summary {
            data.tx_summary = after.tx_summary.clone();
        }
        if before.tx_detail != after.tx_detail {
            data.tx_detail = after.tx_detail.clone();
        }
        if before.categories != after.categories {
            data.categories = after.categories.clone();
        }
        if before.category_labels != after.category_labels {
            data.category_labels = after.category_labels.clone();
        }
        if before.category_colors != after.category_colors {
            data.category_colors = after.category_colors.clone();
        }
        if before.sepa_accounts != after.sepa_accounts {
            data.sepa_accounts = after.sepa_accounts.clone();
        }
        if before.tf_account != after.tf_account {
            data.tf_account = after.tf_account;
        }
        if before.payment_orders != after.payment_orders {
            data.payment_orders = after.payment_orders.clone();
        }
        if before.queued_order_count != after.queued_order_count {
            data.queued_order_count = after.queued_order_count;
        }
        if before.recipient_names != after.recipient_names {
            data.recipient_names = after.recipient_names.clone();
        }
        if before.tf_recipient != after.tf_recipient {
            data.tf_recipient = after.tf_recipient;
        }
        if before.tf_save_recipient != after.tf_save_recipient {
            data.tf_save_recipient = after.tf_save_recipient;
        }
        if before.tf_name != after.tf_name {
            data.tf_name = after.tf_name.clone();
        }
        if before.tf_iban != after.tf_iban {
            data.tf_iban = after.tf_iban.clone();
        }
        if before.tf_bic != after.tf_bic {
            data.tf_bic = after.tf_bic.clone();
        }
        if before.tf_amount != after.tf_amount {
            data.tf_amount = after.tf_amount.clone();
        }
        if before.tf_purpose != after.tf_purpose {
            data.tf_purpose = after.tf_purpose.clone();
        }
        if before.tf_mode != after.tf_mode {
            data.tf_mode = after.tf_mode;
        }
        if before.tf_date != after.tf_date {
            data.tf_date = after.tf_date.clone();
        }
        if before.tf_iban_hint != after.tf_iban_hint {
            data.tf_iban_hint = after.tf_iban_hint.clone();
        }
        if before.tf_iban_error != after.tf_iban_error {
            data.tf_iban_error = after.tf_iban_error;
        }
        if before.so_interval != after.so_interval {
            data.so_interval = after.so_interval;
        }
        if before.so_first != after.so_first {
            data.so_first = after.so_first.clone();
        }
        if before.so_day != after.so_day {
            data.so_day = after.so_day.clone();
        }
        if before.so_last != after.so_last {
            data.so_last = after.so_last.clone();
        }
        if before.standing != after.standing {
            data.standing = after.standing.clone();
        }
        if before.analysis_categories != after.analysis_categories {
            data.analysis_categories = after.analysis_categories.clone();
        }
        if before.analysis_income != after.analysis_income {
            data.analysis_income = after.analysis_income.clone();
        }
        if before.analysis_months != after.analysis_months {
            data.analysis_months = after.analysis_months.clone();
        }
        if before.analysis_label != after.analysis_label {
            data.analysis_label = after.analysis_label.clone();
        }
        if before.analysis_offset != after.analysis_offset {
            data.analysis_offset = after.analysis_offset;
        }
        if before.accesses != after.accesses {
            data.accesses = after.accesses.clone();
        }
        if before.af_open != after.af_open {
            data.af_open = after.af_open;
        }
        if before.af_kind != after.af_kind {
            data.af_kind = after.af_kind;
        }
        if before.af_statement_format != after.af_statement_format {
            data.af_statement_format = after.af_statement_format;
        }
        if before.af_name != after.af_name {
            data.af_name = after.af_name.clone();
        }
        if before.af_blz != after.af_blz {
            data.af_blz = after.af_blz.clone();
        }
        if before.af_user != after.af_user {
            data.af_user = after.af_user.clone();
        }
        if before.af_customer != after.af_customer {
            data.af_customer = after.af_customer.clone();
        }
        if before.af_url != after.af_url {
            data.af_url = after.af_url.clone();
        }
        if before.af_reader != after.af_reader {
            data.af_reader = after.af_reader;
        }
        if before.af_record != after.af_record {
            data.af_record = after.af_record.clone();
        }
        if before.bank_query != after.bank_query {
            data.bank_query = after.bank_query.clone();
        }
        if before.bank_results != after.bank_results {
            data.bank_results = after.bank_results.clone();
        }
        if before.bank_picked != after.bank_picked {
            data.bank_picked = after.bank_picked.clone();
        }
        if before.reader_names != after.reader_names {
            data.reader_names = after.reader_names.clone();
        }
        if before.reader_report != after.reader_report {
            data.reader_report = after.reader_report.clone();
        }
        if before.cyberjack_found != after.cyberjack_found {
            data.cyberjack_found = after.cyberjack_found;
        }
        if before.tan_reader != after.tan_reader {
            data.tan_reader = after.tan_reader;
        }
        if before.theme_mode != after.theme_mode {
            data.theme_mode = after.theme_mode;
        }
        if before.product_id != after.product_id {
            data.product_id = after.product_id.clone();
        }
        if before.product_version != after.product_version {
            data.product_version = after.product_version.clone();
        }
        if before.data_path != after.data_path {
            data.data_path = after.data_path.clone();
        }
        if before.old_password != after.old_password {
            data.old_password = after.old_password.clone();
        }
        if before.new_password != after.new_password {
            data.new_password = after.new_password.clone();
        }
        if before.show_pin != after.show_pin {
            data.show_pin = after.show_pin;
        }
        if before.pin_title != after.pin_title {
            data.pin_title = after.pin_title.clone();
        }
        if before.pin_value != after.pin_value {
            data.pin_value = after.pin_value.clone();
        }
        if before.pin_heading != after.pin_heading {
            data.pin_heading = after.pin_heading.clone();
        }
        if before.pin_masked != after.pin_masked {
            data.pin_masked = after.pin_masked;
        }
        if before.pin_offer_save != after.pin_offer_save {
            data.pin_offer_save = after.pin_offer_save;
        }
        if before.pin_save != after.pin_save {
            data.pin_save = after.pin_save;
        }
        if before.paste_note != after.paste_note {
            data.paste_note = after.paste_note.clone();
        }
        if before.show_pinpad != after.show_pinpad {
            data.show_pinpad = after.show_pinpad;
        }
        if before.pinpad_text != after.pinpad_text {
            data.pinpad_text = after.pinpad_text.clone();
        }
        if before.show_tan != after.show_tan {
            data.show_tan = after.show_tan;
        }
        if before.tan_title != after.tan_title {
            data.tan_title = after.tan_title.clone();
        }
        if before.tan_text != after.tan_text {
            data.tan_text = after.tan_text.clone();
        }
        if before.tan_kind != after.tan_kind {
            data.tan_kind = after.tan_kind;
        }
        if before.tan_value != after.tan_value {
            data.tan_value = after.tan_value.clone();
        }
        if before.tan_image != after.tan_image {
            data.tan_image = after.tan_image.clone();
        }
        if before.tan_usb_possible != after.tan_usb_possible {
            data.tan_usb_possible = after.tan_usb_possible;
        }
        if before.flick0 != after.flick0 {
            data.flick0 = after.flick0;
        }
        if before.flick1 != after.flick1 {
            data.flick1 = after.flick1;
        }
        if before.flick2 != after.flick2 {
            data.flick2 = after.flick2;
        }
        if before.flick3 != after.flick3 {
            data.flick3 = after.flick3;
        }
        if before.flick4 != after.flick4 {
            data.flick4 = after.flick4;
        }
        if before.flicker_width != after.flicker_width {
            data.flicker_width = after.flicker_width;
        }
        if before.flicker_hz != after.flicker_hz {
            data.flicker_hz = after.flicker_hz;
        }
        if before.show_choice != after.show_choice {
            data.show_choice = after.show_choice;
        }
        if before.choice_title != after.choice_title {
            data.choice_title = after.choice_title.clone();
        }
        if before.choices != after.choices {
            data.choices = after.choices.clone();
        }
        if before.show_confirm != after.show_confirm {
            data.show_confirm = after.show_confirm;
        }
        if before.confirm_title != after.confirm_title {
            data.confirm_title = after.confirm_title.clone();
        }
        if before.confirm_text != after.confirm_text {
            data.confirm_text = after.confirm_text.clone();
        }
        if before.confirm_danger != after.confirm_danger {
            data.confirm_danger = after.confirm_danger;
        }
        if before.confirm_ok_text != after.confirm_ok_text {
            data.confirm_ok_text = after.confirm_ok_text.clone();
        }
        if before.show_vop != after.show_vop {
            data.show_vop = after.show_vop;
        }
        if before.vop_status != after.vop_status {
            data.vop_status = after.vop_status;
        }
        if before.vop_name != after.vop_name {
            data.vop_name = after.vop_name.clone();
        }
        if before.vop_iban != after.vop_iban {
            data.vop_iban = after.vop_iban.clone();
        }
        if before.vop_suggested_name != after.vop_suggested_name {
            data.vop_suggested_name = after.vop_suggested_name.clone();
        }
        if before.vop_reason != after.vop_reason {
            data.vop_reason = after.vop_reason.clone();
        }
        if before.vop_notice != after.vop_notice {
            data.vop_notice = after.vop_notice.clone();
        }
        if before.vop_executed != after.vop_executed {
            data.vop_executed = after.vop_executed;
        }
        if before.language_mode != after.language_mode {
            data.language_mode = after.language_mode;
        }
    }
    pub fn get_unlocked(&self) -> bool {
        self.data.borrow().unlocked
    }
    pub fn set_unlocked(&self, value: bool) {
        self.data.borrow_mut().unlocked = value;
    }
    pub fn get_first_run(&self) -> bool {
        self.data.borrow().first_run
    }
    pub fn set_first_run(&self, value: bool) {
        self.data.borrow_mut().first_run = value;
    }
    pub fn get_show_vault_manager(&self) -> bool {
        self.data.borrow().show_vault_manager
    }
    pub fn set_show_vault_manager(&self, value: bool) {
        self.data.borrow_mut().show_vault_manager = value;
    }
    pub fn get_recent_vaults(&self) -> ModelRc<SharedString> {
        self.data.borrow().recent_vaults.clone()
    }
    pub fn set_recent_vaults(&self, value: ModelRc<SharedString>) {
        self.data.borrow_mut().recent_vaults = value;
    }
    pub fn get_selected_vault(&self) -> i32 {
        self.data.borrow().selected_vault
    }
    pub fn set_selected_vault(&self, value: i32) {
        self.data.borrow_mut().selected_vault = value;
    }
    pub fn get_selected_vault_name(&self) -> SharedString {
        self.data.borrow().selected_vault_name.clone()
    }
    pub fn set_selected_vault_name(&self, value: SharedString) {
        self.data.borrow_mut().selected_vault_name = value;
    }
    pub fn get_selected_vault_path(&self) -> SharedString {
        self.data.borrow().selected_vault_path.clone()
    }
    pub fn set_selected_vault_path(&self, value: SharedString) {
        self.data.borrow_mut().selected_vault_path = value;
    }
    pub fn get_active_vault_name(&self) -> SharedString {
        self.data.borrow().active_vault_name.clone()
    }
    pub fn set_active_vault_name(&self, value: SharedString) {
        self.data.borrow_mut().active_vault_name = value;
    }
    pub fn get_vault_selection_revision(&self) -> i32 {
        self.data.borrow().vault_selection_revision
    }
    pub fn set_vault_selection_revision(&self, value: i32) {
        self.data.borrow_mut().vault_selection_revision = value;
    }
    pub fn get_unlock_error(&self) -> SharedString {
        self.data.borrow().unlock_error.clone()
    }
    pub fn set_unlock_error(&self, value: SharedString) {
        self.data.borrow_mut().unlock_error = value;
    }
    pub fn get_page(&self) -> i32 {
        self.data.borrow().page
    }
    pub fn set_page(&self, value: i32) {
        self.data.borrow_mut().page = value;
    }
    pub fn get_busy(&self) -> bool {
        self.data.borrow().busy
    }
    pub fn set_busy(&self, value: bool) {
        self.data.borrow_mut().busy = value;
    }
    pub fn get_busy_text(&self) -> SharedString {
        self.data.borrow().busy_text.clone()
    }
    pub fn set_busy_text(&self, value: SharedString) {
        self.data.borrow_mut().busy_text = value;
    }
    pub fn get_toast(&self) -> Toast {
        self.data.borrow().toast.clone()
    }
    pub fn set_toast(&self, value: Toast) {
        self.data.borrow_mut().toast = value;
    }
    pub fn get_last_sync(&self) -> SharedString {
        self.data.borrow().last_sync.clone()
    }
    pub fn set_last_sync(&self, value: SharedString) {
        self.data.borrow_mut().last_sync = value;
    }
    pub fn get_total_balance(&self) -> SharedString {
        self.data.borrow().total_balance.clone()
    }
    pub fn set_total_balance(&self, value: SharedString) {
        self.data.borrow_mut().total_balance = value;
    }
    pub fn get_total_negative(&self) -> bool {
        self.data.borrow().total_negative
    }
    pub fn set_total_negative(&self, value: bool) {
        self.data.borrow_mut().total_negative = value;
    }
    pub fn get_month_label(&self) -> SharedString {
        self.data.borrow().month_label.clone()
    }
    pub fn set_month_label(&self, value: SharedString) {
        self.data.borrow_mut().month_label = value;
    }
    pub fn get_month_income(&self) -> SharedString {
        self.data.borrow().month_income.clone()
    }
    pub fn set_month_income(&self, value: SharedString) {
        self.data.borrow_mut().month_income = value;
    }
    pub fn get_month_expense(&self) -> SharedString {
        self.data.borrow().month_expense.clone()
    }
    pub fn set_month_expense(&self, value: SharedString) {
        self.data.borrow_mut().month_expense = value;
    }
    pub fn get_accounts(&self) -> ModelRc<AccountRow> {
        self.data.borrow().accounts.clone()
    }
    pub fn set_accounts(&self, value: ModelRc<AccountRow>) {
        self.data.borrow_mut().accounts = value;
    }
    pub fn get_account_groups(&self) -> ModelRc<AccountGroupRow> {
        self.data.borrow().account_groups.clone()
    }
    pub fn set_account_groups(&self, value: ModelRc<AccountGroupRow>) {
        self.data.borrow_mut().account_groups = value;
    }
    pub fn get_group_names(&self) -> ModelRc<SharedString> {
        self.data.borrow().group_names.clone()
    }
    pub fn set_group_names(&self, value: ModelRc<SharedString>) {
        self.data.borrow_mut().group_names = value;
    }
    pub fn get_new_group_name(&self) -> SharedString {
        self.data.borrow().new_group_name.clone()
    }
    pub fn set_new_group_name(&self, value: SharedString) {
        self.data.borrow_mut().new_group_name = value;
    }
    pub fn get_account_names(&self) -> ModelRc<SharedString> {
        self.data.borrow().account_names.clone()
    }
    pub fn set_account_names(&self, value: ModelRc<SharedString>) {
        self.data.borrow_mut().account_names = value;
    }
    pub fn get_recent(&self) -> ModelRc<TxRow> {
        self.data.borrow().recent.clone()
    }
    pub fn set_recent(&self, value: ModelRc<TxRow>) {
        self.data.borrow_mut().recent = value;
    }
    pub fn get_category_stats(&self) -> ModelRc<BarStat> {
        self.data.borrow().category_stats.clone()
    }
    pub fn set_category_stats(&self, value: ModelRc<BarStat>) {
        self.data.borrow_mut().category_stats = value;
    }
    pub fn get_months(&self) -> ModelRc<MonthStat> {
        self.data.borrow().months.clone()
    }
    pub fn set_months(&self, value: ModelRc<MonthStat>) {
        self.data.borrow_mut().months = value;
    }
    pub fn get_upcoming(&self) -> ModelRc<StandingRow> {
        self.data.borrow().upcoming.clone()
    }
    pub fn set_upcoming(&self, value: ModelRc<StandingRow>) {
        self.data.borrow_mut().upcoming = value;
    }
    pub fn get_overview_group_names(&self) -> ModelRc<SharedString> {
        self.data.borrow().overview_group_names.clone()
    }
    pub fn set_overview_group_names(&self, value: ModelRc<SharedString>) {
        self.data.borrow_mut().overview_group_names = value;
    }
    pub fn get_recent_group(&self) -> i32 {
        self.data.borrow().recent_group
    }
    pub fn set_recent_group(&self, value: i32) {
        self.data.borrow_mut().recent_group = value;
    }
    pub fn get_category_group(&self) -> i32 {
        self.data.borrow().category_group
    }
    pub fn set_category_group(&self, value: i32) {
        self.data.borrow_mut().category_group = value;
    }
    pub fn get_months_group(&self) -> i32 {
        self.data.borrow().months_group
    }
    pub fn set_months_group(&self, value: i32) {
        self.data.borrow_mut().months_group = value;
    }
    pub fn get_upcoming_group(&self) -> i32 {
        self.data.borrow().upcoming_group
    }
    pub fn set_upcoming_group(&self, value: i32) {
        self.data.borrow_mut().upcoming_group = value;
    }
    pub fn get_show_account_name(&self) -> bool {
        self.data.borrow().show_account_name
    }
    pub fn set_show_account_name(&self, value: bool) {
        self.data.borrow_mut().show_account_name = value;
    }
    pub fn get_editing_account_id(&self) -> i32 {
        self.data.borrow().editing_account_id
    }
    pub fn set_editing_account_id(&self, value: i32) {
        self.data.borrow_mut().editing_account_id = value;
    }
    pub fn get_account_name_draft(&self) -> SharedString {
        self.data.borrow().account_name_draft.clone()
    }
    pub fn set_account_name_draft(&self, value: SharedString) {
        self.data.borrow_mut().account_name_draft = value;
    }
    pub fn get_account_bank_name(&self) -> SharedString {
        self.data.borrow().account_bank_name.clone()
    }
    pub fn set_account_bank_name(&self, value: SharedString) {
        self.data.borrow_mut().account_bank_name = value;
    }
    pub fn get_account_name_context(&self) -> SharedString {
        self.data.borrow().account_name_context.clone()
    }
    pub fn set_account_name_context(&self, value: SharedString) {
        self.data.borrow_mut().account_name_context = value;
    }
    pub fn get_tx_rows(&self) -> ModelRc<TxRow> {
        self.data.borrow().tx_rows.clone()
    }
    pub fn set_tx_rows(&self, value: ModelRc<TxRow>) {
        self.data.borrow_mut().tx_rows = value;
    }
    pub fn get_tx_account(&self) -> i32 {
        self.data.borrow().tx_account
    }
    pub fn set_tx_account(&self, value: i32) {
        self.data.borrow_mut().tx_account = value;
    }
    pub fn get_tx_search(&self) -> SharedString {
        self.data.borrow().tx_search.clone()
    }
    pub fn set_tx_search(&self, value: SharedString) {
        self.data.borrow_mut().tx_search = value;
    }
    pub fn get_tx_category(&self) -> SharedString {
        self.data.borrow().tx_category.clone()
    }
    pub fn set_tx_category(&self, value: SharedString) {
        self.data.borrow_mut().tx_category = value;
    }
    pub fn get_tx_period(&self) -> i32 {
        self.data.borrow().tx_period
    }
    pub fn set_tx_period(&self, value: i32) {
        self.data.borrow_mut().tx_period = value;
    }
    pub fn get_tx_summary(&self) -> SharedString {
        self.data.borrow().tx_summary.clone()
    }
    pub fn set_tx_summary(&self, value: SharedString) {
        self.data.borrow_mut().tx_summary = value;
    }
    pub fn get_tx_detail(&self) -> TxDetail {
        self.data.borrow().tx_detail.clone()
    }
    pub fn set_tx_detail(&self, value: TxDetail) {
        self.data.borrow_mut().tx_detail = value;
    }
    pub fn get_categories(&self) -> ModelRc<SharedString> {
        self.data.borrow().categories.clone()
    }
    pub fn set_categories(&self, value: ModelRc<SharedString>) {
        self.data.borrow_mut().categories = value;
    }
    pub fn get_category_labels(&self) -> ModelRc<SharedString> {
        self.data.borrow().category_labels.clone()
    }
    pub fn set_category_labels(&self, value: ModelRc<SharedString>) {
        self.data.borrow_mut().category_labels = value;
    }
    pub fn get_category_colors(&self) -> ModelRc<Color> {
        self.data.borrow().category_colors.clone()
    }
    pub fn set_category_colors(&self, value: ModelRc<Color>) {
        self.data.borrow_mut().category_colors = value;
    }
    pub fn get_sepa_accounts(&self) -> ModelRc<SharedString> {
        self.data.borrow().sepa_accounts.clone()
    }
    pub fn set_sepa_accounts(&self, value: ModelRc<SharedString>) {
        self.data.borrow_mut().sepa_accounts = value;
    }
    pub fn get_tf_account(&self) -> i32 {
        self.data.borrow().tf_account
    }
    pub fn set_tf_account(&self, value: i32) {
        self.data.borrow_mut().tf_account = value;
    }
    pub fn get_payment_orders(&self) -> ModelRc<PaymentOrderRow> {
        self.data.borrow().payment_orders.clone()
    }
    pub fn set_payment_orders(&self, value: ModelRc<PaymentOrderRow>) {
        self.data.borrow_mut().payment_orders = value;
    }
    pub fn get_queued_order_count(&self) -> i32 {
        self.data.borrow().queued_order_count
    }
    pub fn set_queued_order_count(&self, value: i32) {
        self.data.borrow_mut().queued_order_count = value;
    }
    pub fn get_recipient_names(&self) -> ModelRc<SharedString> {
        self.data.borrow().recipient_names.clone()
    }
    pub fn set_recipient_names(&self, value: ModelRc<SharedString>) {
        self.data.borrow_mut().recipient_names = value;
    }
    pub fn get_tf_recipient(&self) -> i32 {
        self.data.borrow().tf_recipient
    }
    pub fn set_tf_recipient(&self, value: i32) {
        self.data.borrow_mut().tf_recipient = value;
    }
    pub fn get_tf_save_recipient(&self) -> bool {
        self.data.borrow().tf_save_recipient
    }
    pub fn set_tf_save_recipient(&self, value: bool) {
        self.data.borrow_mut().tf_save_recipient = value;
    }
    pub fn get_tf_name(&self) -> SharedString {
        self.data.borrow().tf_name.clone()
    }
    pub fn set_tf_name(&self, value: SharedString) {
        self.data.borrow_mut().tf_name = value;
    }
    pub fn get_tf_iban(&self) -> SharedString {
        self.data.borrow().tf_iban.clone()
    }
    pub fn set_tf_iban(&self, value: SharedString) {
        self.data.borrow_mut().tf_iban = value;
    }
    pub fn get_tf_bic(&self) -> SharedString {
        self.data.borrow().tf_bic.clone()
    }
    pub fn set_tf_bic(&self, value: SharedString) {
        self.data.borrow_mut().tf_bic = value;
    }
    pub fn get_tf_amount(&self) -> SharedString {
        self.data.borrow().tf_amount.clone()
    }
    pub fn set_tf_amount(&self, value: SharedString) {
        self.data.borrow_mut().tf_amount = value;
    }
    pub fn get_tf_purpose(&self) -> SharedString {
        self.data.borrow().tf_purpose.clone()
    }
    pub fn set_tf_purpose(&self, value: SharedString) {
        self.data.borrow_mut().tf_purpose = value;
    }
    pub fn get_tf_mode(&self) -> i32 {
        self.data.borrow().tf_mode
    }
    pub fn set_tf_mode(&self, value: i32) {
        self.data.borrow_mut().tf_mode = value;
    }
    pub fn get_tf_date(&self) -> SharedString {
        self.data.borrow().tf_date.clone()
    }
    pub fn set_tf_date(&self, value: SharedString) {
        self.data.borrow_mut().tf_date = value;
    }
    pub fn get_tf_iban_hint(&self) -> SharedString {
        self.data.borrow().tf_iban_hint.clone()
    }
    pub fn set_tf_iban_hint(&self, value: SharedString) {
        self.data.borrow_mut().tf_iban_hint = value;
    }
    pub fn get_tf_iban_error(&self) -> bool {
        self.data.borrow().tf_iban_error
    }
    pub fn set_tf_iban_error(&self, value: bool) {
        self.data.borrow_mut().tf_iban_error = value;
    }
    pub fn get_so_interval(&self) -> i32 {
        self.data.borrow().so_interval
    }
    pub fn set_so_interval(&self, value: i32) {
        self.data.borrow_mut().so_interval = value;
    }
    pub fn get_so_first(&self) -> SharedString {
        self.data.borrow().so_first.clone()
    }
    pub fn set_so_first(&self, value: SharedString) {
        self.data.borrow_mut().so_first = value;
    }
    pub fn get_so_day(&self) -> SharedString {
        self.data.borrow().so_day.clone()
    }
    pub fn set_so_day(&self, value: SharedString) {
        self.data.borrow_mut().so_day = value;
    }
    pub fn get_so_last(&self) -> SharedString {
        self.data.borrow().so_last.clone()
    }
    pub fn set_so_last(&self, value: SharedString) {
        self.data.borrow_mut().so_last = value;
    }
    pub fn get_standing(&self) -> ModelRc<StandingRow> {
        self.data.borrow().standing.clone()
    }
    pub fn set_standing(&self, value: ModelRc<StandingRow>) {
        self.data.borrow_mut().standing = value;
    }
    pub fn get_analysis_categories(&self) -> ModelRc<BarStat> {
        self.data.borrow().analysis_categories.clone()
    }
    pub fn set_analysis_categories(&self, value: ModelRc<BarStat>) {
        self.data.borrow_mut().analysis_categories = value;
    }
    pub fn get_analysis_income(&self) -> ModelRc<BarStat> {
        self.data.borrow().analysis_income.clone()
    }
    pub fn set_analysis_income(&self, value: ModelRc<BarStat>) {
        self.data.borrow_mut().analysis_income = value;
    }
    pub fn get_analysis_months(&self) -> ModelRc<MonthStat> {
        self.data.borrow().analysis_months.clone()
    }
    pub fn set_analysis_months(&self, value: ModelRc<MonthStat>) {
        self.data.borrow_mut().analysis_months = value;
    }
    pub fn get_analysis_label(&self) -> SharedString {
        self.data.borrow().analysis_label.clone()
    }
    pub fn set_analysis_label(&self, value: SharedString) {
        self.data.borrow_mut().analysis_label = value;
    }
    pub fn get_analysis_offset(&self) -> i32 {
        self.data.borrow().analysis_offset
    }
    pub fn set_analysis_offset(&self, value: i32) {
        self.data.borrow_mut().analysis_offset = value;
    }
    pub fn get_accesses(&self) -> ModelRc<AccessRow> {
        self.data.borrow().accesses.clone()
    }
    pub fn set_accesses(&self, value: ModelRc<AccessRow>) {
        self.data.borrow_mut().accesses = value;
    }
    pub fn get_af_open(&self) -> bool {
        self.data.borrow().af_open
    }
    pub fn set_af_open(&self, value: bool) {
        self.data.borrow_mut().af_open = value;
    }
    pub fn get_af_kind(&self) -> i32 {
        self.data.borrow().af_kind
    }
    pub fn get_af_statement_format(&self) -> i32 {
        self.data.borrow().af_statement_format
    }
    pub fn set_af_statement_format(&self, value: i32) {
        self.data.borrow_mut().af_statement_format = value;
    }
    pub fn set_af_kind(&self, value: i32) {
        self.data.borrow_mut().af_kind = value;
    }
    pub fn get_af_name(&self) -> SharedString {
        self.data.borrow().af_name.clone()
    }
    pub fn set_af_name(&self, value: SharedString) {
        self.data.borrow_mut().af_name = value;
    }
    pub fn get_af_blz(&self) -> SharedString {
        self.data.borrow().af_blz.clone()
    }
    pub fn set_af_blz(&self, value: SharedString) {
        self.data.borrow_mut().af_blz = value;
    }
    pub fn get_af_user(&self) -> SharedString {
        self.data.borrow().af_user.clone()
    }
    pub fn set_af_user(&self, value: SharedString) {
        self.data.borrow_mut().af_user = value;
    }
    pub fn get_af_customer(&self) -> SharedString {
        self.data.borrow().af_customer.clone()
    }
    pub fn set_af_customer(&self, value: SharedString) {
        self.data.borrow_mut().af_customer = value;
    }
    pub fn get_af_url(&self) -> SharedString {
        self.data.borrow().af_url.clone()
    }
    pub fn set_af_url(&self, value: SharedString) {
        self.data.borrow_mut().af_url = value;
    }
    pub fn get_af_reader(&self) -> i32 {
        self.data.borrow().af_reader
    }
    pub fn set_af_reader(&self, value: i32) {
        self.data.borrow_mut().af_reader = value;
    }
    pub fn get_af_record(&self) -> SharedString {
        self.data.borrow().af_record.clone()
    }
    pub fn set_af_record(&self, value: SharedString) {
        self.data.borrow_mut().af_record = value;
    }
    pub fn get_bank_query(&self) -> SharedString {
        self.data.borrow().bank_query.clone()
    }
    pub fn set_bank_query(&self, value: SharedString) {
        self.data.borrow_mut().bank_query = value;
    }
    pub fn get_bank_results(&self) -> ModelRc<BankRow> {
        self.data.borrow().bank_results.clone()
    }
    pub fn set_bank_results(&self, value: ModelRc<BankRow>) {
        self.data.borrow_mut().bank_results = value;
    }
    pub fn get_bank_picked(&self) -> SharedString {
        self.data.borrow().bank_picked.clone()
    }
    pub fn set_bank_picked(&self, value: SharedString) {
        self.data.borrow_mut().bank_picked = value;
    }
    pub fn get_reader_names(&self) -> ModelRc<SharedString> {
        self.data.borrow().reader_names.clone()
    }
    pub fn set_reader_names(&self, value: ModelRc<SharedString>) {
        self.data.borrow_mut().reader_names = value;
    }
    pub fn get_reader_report(&self) -> ModelRc<SharedString> {
        self.data.borrow().reader_report.clone()
    }
    pub fn set_reader_report(&self, value: ModelRc<SharedString>) {
        self.data.borrow_mut().reader_report = value;
    }
    pub fn get_cyberjack_found(&self) -> bool {
        self.data.borrow().cyberjack_found
    }
    pub fn set_cyberjack_found(&self, value: bool) {
        self.data.borrow_mut().cyberjack_found = value;
    }
    pub fn get_tan_reader(&self) -> i32 {
        self.data.borrow().tan_reader
    }
    pub fn set_tan_reader(&self, value: i32) {
        self.data.borrow_mut().tan_reader = value;
    }
    pub fn get_theme_mode(&self) -> i32 {
        self.data.borrow().theme_mode
    }
    pub fn set_theme_mode(&self, value: i32) {
        self.data.borrow_mut().theme_mode = value;
    }
    pub fn get_product_id(&self) -> SharedString {
        self.data.borrow().product_id.clone()
    }
    pub fn set_product_id(&self, value: SharedString) {
        self.data.borrow_mut().product_id = value;
    }
    pub fn get_product_version(&self) -> SharedString {
        self.data.borrow().product_version.clone()
    }
    pub fn set_product_version(&self, value: SharedString) {
        self.data.borrow_mut().product_version = value;
    }
    pub fn get_data_path(&self) -> SharedString {
        self.data.borrow().data_path.clone()
    }
    pub fn set_data_path(&self, value: SharedString) {
        self.data.borrow_mut().data_path = value;
    }
    pub fn get_old_password(&self) -> SharedString {
        self.data.borrow().old_password.clone()
    }
    pub fn set_old_password(&self, value: SharedString) {
        self.data.borrow_mut().old_password = value;
    }
    pub fn get_new_password(&self) -> SharedString {
        self.data.borrow().new_password.clone()
    }
    pub fn set_new_password(&self, value: SharedString) {
        self.data.borrow_mut().new_password = value;
    }
    pub fn get_show_pin(&self) -> bool {
        self.data.borrow().show_pin
    }
    pub fn set_show_pin(&self, value: bool) {
        self.data.borrow_mut().show_pin = value;
    }
    pub fn get_pin_title(&self) -> SharedString {
        self.data.borrow().pin_title.clone()
    }
    pub fn set_pin_title(&self, value: SharedString) {
        self.data.borrow_mut().pin_title = value;
    }
    pub fn get_pin_value(&self) -> SharedString {
        self.data.borrow().pin_value.clone()
    }
    pub fn set_pin_value(&self, value: SharedString) {
        self.data.borrow_mut().pin_value = value;
    }
    pub fn get_pin_heading(&self) -> SharedString {
        let data = self.data.borrow();
        if data.pin_heading.is_empty() {
            i18n::tr("PIN-Eingabe").into()
        } else {
            data.pin_heading.clone()
        }
    }
    pub fn set_pin_heading(&self, value: SharedString) {
        self.data.borrow_mut().pin_heading = value;
    }
    pub fn get_pin_masked(&self) -> bool {
        self.data.borrow().pin_masked
    }
    pub fn set_pin_masked(&self, value: bool) {
        self.data.borrow_mut().pin_masked = value;
    }
    pub fn get_pin_offer_save(&self) -> bool {
        self.data.borrow().pin_offer_save
    }
    pub fn set_pin_offer_save(&self, value: bool) {
        self.data.borrow_mut().pin_offer_save = value;
    }
    pub fn get_pin_save(&self) -> bool {
        self.data.borrow().pin_save
    }
    pub fn set_pin_save(&self, value: bool) {
        self.data.borrow_mut().pin_save = value;
    }
    pub fn get_paste_note(&self) -> SharedString {
        self.data.borrow().paste_note.clone()
    }
    pub fn set_paste_note(&self, value: SharedString) {
        self.data.borrow_mut().paste_note = value;
    }
    pub fn get_show_pinpad(&self) -> bool {
        self.data.borrow().show_pinpad
    }
    pub fn set_show_pinpad(&self, value: bool) {
        self.data.borrow_mut().show_pinpad = value;
    }
    pub fn get_pinpad_text(&self) -> SharedString {
        self.data.borrow().pinpad_text.clone()
    }
    pub fn set_pinpad_text(&self, value: SharedString) {
        self.data.borrow_mut().pinpad_text = value;
    }
    pub fn get_show_tan(&self) -> bool {
        self.data.borrow().show_tan
    }
    pub fn set_show_tan(&self, value: bool) {
        self.data.borrow_mut().show_tan = value;
    }
    pub fn get_tan_title(&self) -> SharedString {
        self.data.borrow().tan_title.clone()
    }
    pub fn set_tan_title(&self, value: SharedString) {
        self.data.borrow_mut().tan_title = value;
    }
    pub fn get_tan_text(&self) -> SharedString {
        self.data.borrow().tan_text.clone()
    }
    pub fn set_tan_text(&self, value: SharedString) {
        self.data.borrow_mut().tan_text = value;
    }
    pub fn get_tan_kind(&self) -> i32 {
        self.data.borrow().tan_kind
    }
    pub fn set_tan_kind(&self, value: i32) {
        self.data.borrow_mut().tan_kind = value;
    }
    pub fn get_tan_value(&self) -> SharedString {
        self.data.borrow().tan_value.clone()
    }
    pub fn set_tan_value(&self, value: SharedString) {
        self.data.borrow_mut().tan_value = value;
    }
    pub fn get_tan_image(&self) -> Image {
        self.data.borrow().tan_image.clone()
    }
    pub fn set_tan_image(&self, value: Image) {
        self.data.borrow_mut().tan_image = value;
    }
    pub fn get_tan_usb_possible(&self) -> bool {
        self.data.borrow().tan_usb_possible
    }
    pub fn set_tan_usb_possible(&self, value: bool) {
        self.data.borrow_mut().tan_usb_possible = value;
    }
    pub fn get_flick0(&self) -> bool {
        self.data.borrow().flick0
    }
    pub fn set_flick0(&self, value: bool) {
        self.data.borrow_mut().flick0 = value;
    }
    pub fn get_flick1(&self) -> bool {
        self.data.borrow().flick1
    }
    pub fn set_flick1(&self, value: bool) {
        self.data.borrow_mut().flick1 = value;
    }
    pub fn get_flick2(&self) -> bool {
        self.data.borrow().flick2
    }
    pub fn set_flick2(&self, value: bool) {
        self.data.borrow_mut().flick2 = value;
    }
    pub fn get_flick3(&self) -> bool {
        self.data.borrow().flick3
    }
    pub fn set_flick3(&self, value: bool) {
        self.data.borrow_mut().flick3 = value;
    }
    pub fn get_flick4(&self) -> bool {
        self.data.borrow().flick4
    }
    pub fn set_flick4(&self, value: bool) {
        self.data.borrow_mut().flick4 = value;
    }
    pub fn get_flicker_width(&self) -> f32 {
        self.data.borrow().flicker_width
    }
    pub fn set_flicker_width(&self, value: f32) {
        self.data.borrow_mut().flicker_width = value;
    }
    pub fn get_flicker_hz(&self) -> i32 {
        self.data.borrow().flicker_hz
    }
    pub fn set_flicker_hz(&self, value: i32) {
        self.data.borrow_mut().flicker_hz = value;
    }
    pub fn get_show_choice(&self) -> bool {
        self.data.borrow().show_choice
    }
    pub fn set_show_choice(&self, value: bool) {
        self.data.borrow_mut().show_choice = value;
    }
    pub fn get_choice_title(&self) -> SharedString {
        self.data.borrow().choice_title.clone()
    }
    pub fn set_choice_title(&self, value: SharedString) {
        self.data.borrow_mut().choice_title = value;
    }
    pub fn get_choices(&self) -> ModelRc<ChoiceRow> {
        self.data.borrow().choices.clone()
    }
    pub fn set_choices(&self, value: ModelRc<ChoiceRow>) {
        self.data.borrow_mut().choices = value;
    }
    pub fn get_show_confirm(&self) -> bool {
        self.data.borrow().show_confirm
    }
    pub fn set_show_confirm(&self, value: bool) {
        self.data.borrow_mut().show_confirm = value;
    }
    pub fn get_confirm_title(&self) -> SharedString {
        self.data.borrow().confirm_title.clone()
    }
    pub fn set_confirm_title(&self, value: SharedString) {
        self.data.borrow_mut().confirm_title = value;
    }
    pub fn get_confirm_text(&self) -> SharedString {
        self.data.borrow().confirm_text.clone()
    }
    pub fn set_confirm_text(&self, value: SharedString) {
        self.data.borrow_mut().confirm_text = value;
    }
    pub fn get_confirm_danger(&self) -> bool {
        self.data.borrow().confirm_danger
    }
    pub fn set_confirm_danger(&self, value: bool) {
        self.data.borrow_mut().confirm_danger = value;
    }
    pub fn get_confirm_ok_text(&self) -> SharedString {
        self.data.borrow().confirm_ok_text.clone()
    }
    pub fn set_confirm_ok_text(&self, value: SharedString) {
        self.data.borrow_mut().confirm_ok_text = value;
    }
    pub fn get_show_vop(&self) -> bool {
        self.data.borrow().show_vop
    }
    pub fn set_show_vop(&self, value: bool) {
        self.data.borrow_mut().show_vop = value;
    }
    pub fn get_vop_status(&self) -> i32 {
        self.data.borrow().vop_status
    }
    pub fn set_vop_status(&self, value: i32) {
        self.data.borrow_mut().vop_status = value;
    }
    pub fn get_vop_name(&self) -> SharedString {
        self.data.borrow().vop_name.clone()
    }
    pub fn set_vop_name(&self, value: SharedString) {
        self.data.borrow_mut().vop_name = value;
    }
    pub fn get_vop_iban(&self) -> SharedString {
        self.data.borrow().vop_iban.clone()
    }
    pub fn set_vop_iban(&self, value: SharedString) {
        self.data.borrow_mut().vop_iban = value;
    }
    pub fn get_vop_suggested_name(&self) -> SharedString {
        self.data.borrow().vop_suggested_name.clone()
    }
    pub fn set_vop_suggested_name(&self, value: SharedString) {
        self.data.borrow_mut().vop_suggested_name = value;
    }
    pub fn get_vop_reason(&self) -> SharedString {
        self.data.borrow().vop_reason.clone()
    }
    pub fn set_vop_reason(&self, value: SharedString) {
        self.data.borrow_mut().vop_reason = value;
    }
    pub fn get_vop_notice(&self) -> SharedString {
        self.data.borrow().vop_notice.clone()
    }
    pub fn set_vop_notice(&self, value: SharedString) {
        self.data.borrow_mut().vop_notice = value;
    }
    pub fn get_vop_executed(&self) -> bool {
        self.data.borrow().vop_executed
    }
    pub fn set_vop_executed(&self, value: bool) {
        self.data.borrow_mut().vop_executed = value;
    }
    pub fn get_language_mode(&self) -> i32 {
        self.data.borrow().language_mode
    }
    pub fn set_language_mode(&self, value: i32) {
        self.data.borrow_mut().language_mode = value;
    }
    pub fn on_vop_accept(&self, callback: impl Fn() + 'static) {
        self.callbacks.borrow_mut().vop_accept = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_vop_accept(&self) {
        let callback = self.callbacks.borrow().vop_accept.clone();
        callback.map(|f| f()).unwrap_or_default()
    }
    pub fn on_vop_cancel(&self, callback: impl Fn() + 'static) {
        self.callbacks.borrow_mut().vop_cancel = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_vop_cancel(&self) {
        let callback = self.callbacks.borrow().vop_cancel.clone();
        callback.map(|f| f()).unwrap_or_default()
    }
    pub fn on_vop_copy_name(&self, callback: impl Fn() + 'static) {
        self.callbacks.borrow_mut().vop_copy_name = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_vop_copy_name(&self) {
        let callback = self.callbacks.borrow().vop_copy_name.clone();
        callback.map(|f| f()).unwrap_or_default()
    }
    pub fn on_unlock(&self, callback: impl Fn(SharedString) + 'static) {
        self.callbacks.borrow_mut().unlock = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_unlock(&self, a0: SharedString) {
        let callback = self.callbacks.borrow().unlock.clone();
        callback.map(|f| f(a0)).unwrap_or_default()
    }
    pub fn on_create_vault(&self, callback: impl Fn(SharedString, SharedString) + 'static) {
        self.callbacks.borrow_mut().create_vault = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_create_vault(&self, a0: SharedString, a1: SharedString) {
        let callback = self.callbacks.borrow().create_vault.clone();
        callback.map(|f| f(a0, a1)).unwrap_or_default()
    }
    pub fn on_manage_vaults(&self, callback: impl Fn() + 'static) {
        self.callbacks.borrow_mut().manage_vaults = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_manage_vaults(&self) {
        let callback = self.callbacks.borrow().manage_vaults.clone();
        callback.map(|f| f()).unwrap_or_default()
    }
    pub fn on_cancel_vault_selection(&self, callback: impl Fn() + 'static) {
        self.callbacks.borrow_mut().cancel_vault_selection = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_cancel_vault_selection(&self) {
        let callback = self.callbacks.borrow().cancel_vault_selection.clone();
        callback.map(|f| f()).unwrap_or_default()
    }
    pub fn on_choose_vault(&self, callback: impl Fn(i32) + 'static) {
        self.callbacks.borrow_mut().choose_vault = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_choose_vault(&self, a0: i32) {
        let callback = self.callbacks.borrow().choose_vault.clone();
        callback.map(|f| f(a0)).unwrap_or_default()
    }
    pub fn on_open_vault_file(&self, callback: impl Fn() + 'static) {
        self.callbacks.borrow_mut().open_vault_file = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_open_vault_file(&self) {
        let callback = self.callbacks.borrow().open_vault_file.clone();
        callback.map(|f| f()).unwrap_or_default()
    }
    pub fn on_new_vault_file(&self, callback: impl Fn() + 'static) {
        self.callbacks.borrow_mut().new_vault_file = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_new_vault_file(&self) {
        let callback = self.callbacks.borrow().new_vault_file.clone();
        callback.map(|f| f()).unwrap_or_default()
    }
    pub fn on_navigate(&self, callback: impl Fn(i32) + 'static) {
        self.callbacks.borrow_mut().navigate = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_navigate(&self, a0: i32) {
        let callback = self.callbacks.borrow().navigate.clone();
        callback.map(|f| f(a0)).unwrap_or_default()
    }
    pub fn on_refresh_all(&self, callback: impl Fn() + 'static) {
        self.callbacks.borrow_mut().refresh_all = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_refresh_all(&self) {
        let callback = self.callbacks.borrow().refresh_all.clone();
        callback.map(|f| f()).unwrap_or_default()
    }
    pub fn on_refresh_transactions(&self, callback: impl Fn() + 'static) {
        self.callbacks.borrow_mut().refresh_transactions = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_refresh_transactions(&self) {
        let callback = self.callbacks.borrow().refresh_transactions.clone();
        callback.map(|f| f()).unwrap_or_default()
    }
    pub fn on_refresh_group(&self, callback: impl Fn(i32) + 'static) {
        self.callbacks.borrow_mut().refresh_group = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_refresh_group(&self, a0: i32) {
        let callback = self.callbacks.borrow().refresh_group.clone();
        callback.map(|f| f(a0)).unwrap_or_default()
    }
    pub fn on_overview_group_changed(&self, callback: impl Fn(i32, i32) + 'static) {
        self.callbacks.borrow_mut().overview_group_changed = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_overview_group_changed(&self, a0: i32, a1: i32) {
        let callback = self.callbacks.borrow().overview_group_changed.clone();
        callback.map(|f| f(a0, a1)).unwrap_or_default()
    }
    pub fn on_open_account(&self, callback: impl Fn(i32) + 'static) {
        self.callbacks.borrow_mut().open_account = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_open_account(&self, a0: i32) {
        let callback = self.callbacks.borrow().open_account.clone();
        callback.map(|f| f(a0)).unwrap_or_default()
    }
    pub fn on_edit_account_name(&self, callback: impl Fn(i32) + 'static) {
        self.callbacks.borrow_mut().edit_account_name = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_edit_account_name(&self, a0: i32) {
        let callback = self.callbacks.borrow().edit_account_name.clone();
        callback.map(|f| f(a0)).unwrap_or_default()
    }
    pub fn on_save_account_name(&self, callback: impl Fn() + 'static) {
        self.callbacks.borrow_mut().save_account_name = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_save_account_name(&self) {
        let callback = self.callbacks.borrow().save_account_name.clone();
        callback.map(|f| f()).unwrap_or_default()
    }
    pub fn on_account_row_layout(&self, callback: impl Fn(i32, f32, f32) + 'static) {
        self.callbacks.borrow_mut().account_row_layout = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_account_row_layout(&self, a0: i32, a1: f32, a2: f32) {
        let callback = self.callbacks.borrow().account_row_layout.clone();
        callback.map(|f| f(a0, a1, a2)).unwrap_or_default()
    }
    pub fn on_account_drop_target(&self, callback: impl Fn(f32) -> AccountDropTarget + 'static) {
        self.callbacks.borrow_mut().account_drop_target = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_account_drop_target(&self, a0: f32) -> AccountDropTarget {
        let callback = self.callbacks.borrow().account_drop_target.clone();
        callback.map(|f| f(a0)).unwrap_or_default()
    }
    pub fn on_create_group(&self, callback: impl Fn(SharedString) + 'static) {
        self.callbacks.borrow_mut().create_group = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_create_group(&self, a0: SharedString) {
        let callback = self.callbacks.borrow().create_group.clone();
        callback.map(|f| f(a0)).unwrap_or_default()
    }
    pub fn on_rename_group(&self, callback: impl Fn(i32, SharedString) + 'static) {
        self.callbacks.borrow_mut().rename_group = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_rename_group(&self, a0: i32, a1: SharedString) {
        let callback = self.callbacks.borrow().rename_group.clone();
        callback.map(|f| f(a0, a1)).unwrap_or_default()
    }
    pub fn on_delete_group(&self, callback: impl Fn(i32) + 'static) {
        self.callbacks.borrow_mut().delete_group = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_delete_group(&self, a0: i32) {
        let callback = self.callbacks.borrow().delete_group.clone();
        callback.map(|f| f(a0)).unwrap_or_default()
    }
    pub fn on_assign_group(&self, callback: impl Fn(i32, i32) + 'static) {
        self.callbacks.borrow_mut().assign_group = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_assign_group(&self, a0: i32, a1: i32) {
        let callback = self.callbacks.borrow().assign_group.clone();
        callback.map(|f| f(a0, a1)).unwrap_or_default()
    }
    pub fn on_move_account(&self, callback: impl Fn(i32, i32, bool) -> bool + 'static) {
        self.callbacks.borrow_mut().move_account = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_move_account(&self, a0: i32, a1: i32, a2: bool) -> bool {
        let callback = self.callbacks.borrow().move_account.clone();
        callback.map(|f| f(a0, a1, a2)).unwrap_or_default()
    }
    pub fn on_tx_filter_changed(&self, callback: impl Fn() + 'static) {
        self.callbacks.borrow_mut().tx_filter_changed = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_tx_filter_changed(&self) {
        let callback = self.callbacks.borrow().tx_filter_changed.clone();
        callback.map(|f| f()).unwrap_or_default()
    }
    pub fn on_tx_select(&self, callback: impl Fn(i32) + 'static) {
        self.callbacks.borrow_mut().tx_select = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_tx_select(&self, a0: i32) {
        let callback = self.callbacks.borrow().tx_select.clone();
        callback.map(|f| f(a0)).unwrap_or_default()
    }
    pub fn on_set_category(&self, callback: impl Fn(i32, SharedString) + 'static) {
        self.callbacks.borrow_mut().set_category = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_set_category(&self, a0: i32, a1: SharedString) {
        let callback = self.callbacks.borrow().set_category.clone();
        callback.map(|f| f(a0, a1)).unwrap_or_default()
    }
    pub fn on_export_csv(&self, callback: impl Fn() + 'static) {
        self.callbacks.borrow_mut().export_csv = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_export_csv(&self) {
        let callback = self.callbacks.borrow().export_csv.clone();
        callback.map(|f| f()).unwrap_or_default()
    }
    pub fn on_export_camt(&self, callback: impl Fn() + 'static) {
        self.callbacks.borrow_mut().export_camt = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_export_camt(&self) {
        let callback = self.callbacks.borrow().export_camt.clone();
        callback.map(|f| f()).unwrap_or_default()
    }
    pub fn on_check_iban(&self, callback: impl Fn(SharedString) + 'static) {
        self.callbacks.borrow_mut().check_iban = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_check_iban(&self, a0: SharedString) {
        let callback = self.callbacks.borrow().check_iban.clone();
        callback.map(|f| f(a0)).unwrap_or_default()
    }
    pub fn on_send_transfer(&self, callback: impl Fn() + 'static) {
        self.callbacks.borrow_mut().send_transfer = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_send_transfer(&self) {
        let callback = self.callbacks.borrow().send_transfer.clone();
        callback.map(|f| f()).unwrap_or_default()
    }
    pub fn on_save_transfer(&self, callback: impl Fn() + 'static) {
        self.callbacks.borrow_mut().save_transfer = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_save_transfer(&self) {
        let callback = self.callbacks.borrow().save_transfer.clone();
        callback.map(|f| f()).unwrap_or_default()
    }
    pub fn on_delete_payment_order(&self, callback: impl Fn(i32) + 'static) {
        self.callbacks.borrow_mut().delete_payment_order = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_delete_payment_order(&self, a0: i32) {
        let callback = self.callbacks.borrow().delete_payment_order.clone();
        callback.map(|f| f(a0)).unwrap_or_default()
    }
    pub fn on_payment_account_changed(&self, callback: impl Fn() + 'static) {
        self.callbacks.borrow_mut().payment_account_changed = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_payment_account_changed(&self) {
        let callback = self.callbacks.borrow().payment_account_changed.clone();
        callback.map(|f| f()).unwrap_or_default()
    }
    pub fn on_select_recipient(&self, callback: impl Fn(i32) + 'static) {
        self.callbacks.borrow_mut().select_recipient = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_select_recipient(&self, a0: i32) {
        let callback = self.callbacks.borrow().select_recipient.clone();
        callback.map(|f| f(a0)).unwrap_or_default()
    }
    pub fn on_delete_recipient(&self, callback: impl Fn() + 'static) {
        self.callbacks.borrow_mut().delete_recipient = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_delete_recipient(&self) {
        let callback = self.callbacks.borrow().delete_recipient.clone();
        callback.map(|f| f()).unwrap_or_default()
    }
    pub fn on_send_standing(&self, callback: impl Fn() + 'static) {
        self.callbacks.borrow_mut().send_standing = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_send_standing(&self) {
        let callback = self.callbacks.borrow().send_standing.clone();
        callback.map(|f| f()).unwrap_or_default()
    }
    pub fn on_fetch_standing(&self, callback: impl Fn() + 'static) {
        self.callbacks.borrow_mut().fetch_standing = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_fetch_standing(&self) {
        let callback = self.callbacks.borrow().fetch_standing.clone();
        callback.map(|f| f()).unwrap_or_default()
    }
    pub fn on_delete_standing(&self, callback: impl Fn(i32) + 'static) {
        self.callbacks.borrow_mut().delete_standing = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_delete_standing(&self, a0: i32) {
        let callback = self.callbacks.borrow().delete_standing.clone();
        callback.map(|f| f(a0)).unwrap_or_default()
    }
    pub fn on_new_access(&self, callback: impl Fn() + 'static) {
        self.callbacks.borrow_mut().new_access = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_new_access(&self) {
        let callback = self.callbacks.borrow().new_access.clone();
        callback.map(|f| f()).unwrap_or_default()
    }
    pub fn on_save_access(&self, callback: impl Fn() + 'static) {
        self.callbacks.borrow_mut().save_access = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_save_access(&self) {
        let callback = self.callbacks.borrow().save_access.clone();
        callback.map(|f| f()).unwrap_or_default()
    }
    pub fn on_delete_access(&self, callback: impl Fn(i32) + 'static) {
        self.callbacks.borrow_mut().delete_access = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_delete_access(&self, a0: i32) {
        let callback = self.callbacks.borrow().delete_access.clone();
        callback.map(|f| f(a0)).unwrap_or_default()
    }
    pub fn on_sync_access(&self, callback: impl Fn(i32) + 'static) {
        self.callbacks.borrow_mut().sync_access = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_sync_access(&self, a0: i32) {
        let callback = self.callbacks.borrow().sync_access.clone();
        callback.map(|f| f(a0)).unwrap_or_default()
    }
    pub fn on_reset_tan(&self, callback: impl Fn(i32) + 'static) {
        self.callbacks.borrow_mut().reset_tan = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_reset_tan(&self, a0: i32) {
        let callback = self.callbacks.borrow().reset_tan.clone();
        callback.map(|f| f(a0)).unwrap_or_default()
    }
    pub fn on_forget_pin(&self, callback: impl Fn(i32) + 'static) {
        self.callbacks.borrow_mut().forget_pin = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_forget_pin(&self, a0: i32) {
        let callback = self.callbacks.borrow().forget_pin.clone();
        callback.map(|f| f(a0)).unwrap_or_default()
    }
    pub fn on_read_card(&self, callback: impl Fn() + 'static) {
        self.callbacks.borrow_mut().read_card = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_read_card(&self) {
        let callback = self.callbacks.borrow().read_card.clone();
        callback.map(|f| f()).unwrap_or_default()
    }
    pub fn on_bank_search(&self, callback: impl Fn(SharedString) + 'static) {
        self.callbacks.borrow_mut().bank_search = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_bank_search(&self, a0: SharedString) {
        let callback = self.callbacks.borrow().bank_search.clone();
        callback.map(|f| f(a0)).unwrap_or_default()
    }
    pub fn on_bank_pick(&self, callback: impl Fn(i32) + 'static) {
        self.callbacks.borrow_mut().bank_pick = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_bank_pick(&self, a0: i32) {
        let callback = self.callbacks.borrow().bank_pick.clone();
        callback.map(|f| f(a0)).unwrap_or_default()
    }
    pub fn on_reader_refresh(&self, callback: impl Fn() + 'static) {
        self.callbacks.borrow_mut().reader_refresh = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_reader_refresh(&self) {
        let callback = self.callbacks.borrow().reader_refresh.clone();
        callback.map(|f| f()).unwrap_or_default()
    }
    pub fn on_reader_diagnose(&self, callback: impl Fn() + 'static) {
        self.callbacks.borrow_mut().reader_diagnose = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_reader_diagnose(&self) {
        let callback = self.callbacks.borrow().reader_diagnose.clone();
        callback.map(|f| f()).unwrap_or_default()
    }
    pub fn on_save_settings(&self, callback: impl Fn() + 'static) {
        self.callbacks.borrow_mut().save_settings = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_save_settings(&self) {
        let callback = self.callbacks.borrow().save_settings.clone();
        callback.map(|f| f()).unwrap_or_default()
    }
    pub fn on_change_theme(&self, callback: impl Fn(i32) + 'static) {
        self.callbacks.borrow_mut().change_theme = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_change_theme(&self, a0: i32) {
        let callback = self.callbacks.borrow().change_theme.clone();
        callback.map(|f| f(a0)).unwrap_or_default()
    }
    pub fn on_change_language(&self, callback: impl Fn(i32) + 'static) {
        self.callbacks.borrow_mut().change_language = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_change_language(&self, a0: i32) {
        let callback = self.callbacks.borrow().change_language.clone();
        callback.map(|f| f(a0)).unwrap_or_default()
    }
    pub fn on_load_demo(&self, callback: impl Fn() + 'static) {
        self.callbacks.borrow_mut().load_demo = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_load_demo(&self) {
        let callback = self.callbacks.borrow().load_demo.clone();
        callback.map(|f| f()).unwrap_or_default()
    }
    pub fn on_change_password(&self, callback: impl Fn() + 'static) {
        self.callbacks.borrow_mut().change_password = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_change_password(&self) {
        let callback = self.callbacks.borrow().change_password.clone();
        callback.map(|f| f()).unwrap_or_default()
    }
    pub fn on_analysis_shift(&self, callback: impl Fn(i32) + 'static) {
        self.callbacks.borrow_mut().analysis_shift = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_analysis_shift(&self, a0: i32) {
        let callback = self.callbacks.borrow().analysis_shift.clone();
        callback.map(|f| f(a0)).unwrap_or_default()
    }
    pub fn on_pin_ok(&self, callback: impl Fn() + 'static) {
        self.callbacks.borrow_mut().pin_ok = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_pin_ok(&self) {
        let callback = self.callbacks.borrow().pin_ok.clone();
        callback.map(|f| f()).unwrap_or_default()
    }
    pub fn on_pin_paste(&self, callback: impl Fn() + 'static) {
        self.callbacks.borrow_mut().pin_paste = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_pin_paste(&self) {
        let callback = self.callbacks.borrow().pin_paste.clone();
        callback.map(|f| f()).unwrap_or_default()
    }
    pub fn on_tan_paste(&self, callback: impl Fn() + 'static) {
        self.callbacks.borrow_mut().tan_paste = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_tan_paste(&self) {
        let callback = self.callbacks.borrow().tan_paste.clone();
        callback.map(|f| f()).unwrap_or_default()
    }
    pub fn on_pin_cancel(&self, callback: impl Fn() + 'static) {
        self.callbacks.borrow_mut().pin_cancel = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_pin_cancel(&self) {
        let callback = self.callbacks.borrow().pin_cancel.clone();
        callback.map(|f| f()).unwrap_or_default()
    }
    pub fn on_tan_ok(&self, callback: impl Fn() + 'static) {
        self.callbacks.borrow_mut().tan_ok = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_tan_ok(&self) {
        let callback = self.callbacks.borrow().tan_ok.clone();
        callback.map(|f| f()).unwrap_or_default()
    }
    pub fn on_tan_cancel(&self, callback: impl Fn() + 'static) {
        self.callbacks.borrow_mut().tan_cancel = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_tan_cancel(&self) {
        let callback = self.callbacks.borrow().tan_cancel.clone();
        callback.map(|f| f()).unwrap_or_default()
    }
    pub fn on_tan_confirmed(&self, callback: impl Fn() + 'static) {
        self.callbacks.borrow_mut().tan_confirmed = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_tan_confirmed(&self) {
        let callback = self.callbacks.borrow().tan_confirmed.clone();
        callback.map(|f| f()).unwrap_or_default()
    }
    pub fn on_tan_usb(&self, callback: impl Fn() + 'static) {
        self.callbacks.borrow_mut().tan_usb = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_tan_usb(&self) {
        let callback = self.callbacks.borrow().tan_usb.clone();
        callback.map(|f| f()).unwrap_or_default()
    }
    pub fn on_flicker_speed(&self, callback: impl Fn(i32) + 'static) {
        self.callbacks.borrow_mut().flicker_speed = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_flicker_speed(&self, a0: i32) {
        let callback = self.callbacks.borrow().flicker_speed.clone();
        callback.map(|f| f(a0)).unwrap_or_default()
    }
    pub fn on_choice_picked(&self, callback: impl Fn(SharedString) + 'static) {
        self.callbacks.borrow_mut().choice_picked = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_choice_picked(&self, a0: SharedString) {
        let callback = self.callbacks.borrow().choice_picked.clone();
        callback.map(|f| f(a0)).unwrap_or_default()
    }
    pub fn on_choice_cancel(&self, callback: impl Fn() + 'static) {
        self.callbacks.borrow_mut().choice_cancel = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_choice_cancel(&self) {
        let callback = self.callbacks.borrow().choice_cancel.clone();
        callback.map(|f| f()).unwrap_or_default()
    }
    pub fn on_confirm_ok(&self, callback: impl Fn() + 'static) {
        self.callbacks.borrow_mut().confirm_ok = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_confirm_ok(&self) {
        let callback = self.callbacks.borrow().confirm_ok.clone();
        callback.map(|f| f()).unwrap_or_default()
    }
    pub fn on_confirm_cancel(&self, callback: impl Fn() + 'static) {
        self.callbacks.borrow_mut().confirm_cancel = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_confirm_cancel(&self) {
        let callback = self.callbacks.borrow().confirm_cancel.clone();
        callback.map(|f| f()).unwrap_or_default()
    }
    pub fn on_toast_close(&self, callback: impl Fn() + 'static) {
        self.callbacks.borrow_mut().toast_close = Some(std::rc::Rc::new(callback));
    }
    pub fn invoke_toast_close(&self) {
        let callback = self.callbacks.borrow().toast_close.clone();
        callback.map(|f| f()).unwrap_or_default()
    }
}
