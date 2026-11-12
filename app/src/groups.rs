use crate::store::{AccountGroup, Vault};

impl Vault {
    pub fn move_account(&mut self, source: u32, target: u32, after: bool) -> Result<(), String> {
        let from = self.accounts.iter().position(|a| a.id == source).ok_or("Konto nicht gefunden")?;
        let target_index = self.accounts.iter().position(|a| a.id == target).ok_or("Zielkonto nicht gefunden")?;
        if source == target { return Ok(()); }
        let mut to = target_index + usize::from(after);
        if from < to { to -= 1; }
        let account = self.accounts.remove(from);
        self.accounts.insert(to, account);
        Ok(())
    }

    fn group_name(&self, name: &str, except: Option<u32>) -> Result<String, String> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 60 || name.chars().any(char::is_control) {
            return Err("Bitte einen Gruppennamen mit 1 bis 60 Zeichen eingeben.".into());
        }
        if name.to_lowercase() == "ohne gruppe" || self.groups.iter().any(|g| Some(g.id) != except && g.name.to_lowercase() == name.to_lowercase()) {
            return Err("Dieser Gruppenname ist bereits vergeben.".into());
        }
        Ok(name.to_string())
    }

    pub fn create_group(&mut self, name: &str) -> Result<u32, String> {
        let name = self.group_name(name, None)?;
        let id = self.new_id();
        self.groups.push(AccountGroup { id, name, account_ids: Vec::new() });
        Ok(id)
    }

    pub fn rename_group(&mut self, id: u32, name: &str) -> Result<(), String> {
        let name = self.group_name(name, Some(id))?;
        self.groups.iter_mut().find(|g| g.id == id).ok_or("Gruppe nicht gefunden")?.name = name;
        Ok(())
    }

    pub fn assign_group(&mut self, account_id: u32, group_id: Option<u32>) -> Result<(), String> {
        if self.account(account_id).is_none() { return Err("Konto nicht gefunden".into()); }
        if group_id.is_some_and(|id| !self.groups.iter().any(|g| g.id == id)) {
            return Err("Gruppe nicht gefunden".into());
        }
        for group in &mut self.groups {
            group.account_ids.retain(|id| *id != account_id);
            if Some(group.id) == group_id { group.account_ids.push(account_id); }
        }
        Ok(())
    }

    pub fn delete_group(&mut self, id: u32) -> Result<(), String> {
        if !self.groups.iter().any(|g| g.id == id) { return Err("Gruppe nicht gefunden".into()); }
        self.groups.retain(|g| g.id != id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn moving_accounts_preserves_groups_and_order_after_refresh_and_reload() {
        let mut v = Vault::default();
        let ids = v.merge_accounts(1, (0..3).map(|i| fints::Account { account_number: i.to_string(), ..Default::default() }).collect());
        let group = v.create_group("Privat").unwrap();
        v.assign_group(ids[0], Some(group)).unwrap();
        v.move_account(ids[0], ids[2], true).unwrap();
        assert_eq!(v.accounts.iter().map(|a| a.id).collect::<Vec<_>>(), vec![ids[1],ids[2],ids[0]]);
        v.move_account(ids[0], ids[1], false).unwrap();
        assert_eq!(v.accounts.iter().map(|a| a.id).collect::<Vec<_>>(), ids);
        v.move_account(ids[2], ids[0], false).unwrap();
        v.merge_accounts(1, vec![fints::Account { account_number: "0".into(), ..Default::default() }]);
        let reloaded: Vault = serde_json::from_slice(&serde_json::to_vec(&v).unwrap()).unwrap();
        assert_eq!(reloaded.accounts.iter().map(|a| a.id).collect::<Vec<_>>(), vec![ids[2],ids[0],ids[1]]);
        assert_eq!(reloaded.groups[0].account_ids, vec![ids[0]]);
        assert!(v.move_account(ids[0], 999, true).is_err());
        assert_eq!(v.accounts.len(), 3);
    }
    #[test]
    fn groups_preserve_accounts_and_survive_serialization() {
        let mut v = Vault::default();
        let account = v.merge_accounts(1, vec![fints::Account { iban: "DE89370400440532013000".into(), ..Default::default() }])[0];
        let private = v.create_group("Privat").unwrap();
        let bank = v.create_group("VR Bank").unwrap();
        assert!(v.create_group(" privat ").is_err());
        v.assign_group(account, Some(private)).unwrap();
        v.assign_group(account, Some(bank)).unwrap();
        assert!(v.groups[0].account_ids.is_empty());
        assert_eq!(v.groups[1].account_ids, vec![account]);
        assert!(v.assign_group(account, Some(999)).is_err());
        v.rename_group(bank, "Bankkonten").unwrap();
        let mut restored: Vault = serde_json::from_slice(&serde_json::to_vec(&v).unwrap()).unwrap();
        assert_eq!(restored.groups[1].name, "Bankkonten");
        restored.delete_group(bank).unwrap();
        assert!(restored.account(account).is_some());
        assert!(restored.groups.iter().all(|g| g.account_ids.is_empty()));
        let mut legacy = serde_json::to_value(v).unwrap();
        legacy.as_object_mut().unwrap().remove("groups");
        assert!(serde_json::from_value::<Vault>(legacy).unwrap().groups.is_empty());
    }
}
