use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum CaseState {
    Active,
    TornDown,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Case {
    pub id: String,
    pub data_bucket: String,
    pub site_bucket: String,
    pub scoped_access_key: String,
    pub site_url: String,
    pub state: CaseState,
}

impl Case {
    fn path(cases_dir: &str, id: &str) -> std::path::PathBuf {
        Path::new(cases_dir).join(format!("{id}.json"))
    }

    pub fn save(&self, cases_dir: &str) -> anyhow::Result<()> {
        std::fs::create_dir_all(cases_dir)?;
        let json = serde_json::to_vec_pretty(self)?;
        std::fs::write(Self::path(cases_dir, &self.id), json)?;
        Ok(())
    }

    pub fn load(cases_dir: &str, id: &str) -> anyhow::Result<Case> {
        let bytes = std::fs::read(Self::path(cases_dir, id))?;
        Ok(serde_json::from_slice(&bytes)?)
    }
}
