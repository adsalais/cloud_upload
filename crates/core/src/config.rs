#[derive(Clone, Debug)]
pub struct Config {
    pub endpoint: String,
    pub region: String,
    pub admin_access_key: String,
    pub admin_secret_key: String,
    pub mc_alias: String,
    pub mc_parent: String,
    pub site_dir: String,
    pub cases_dir: String,
}

impl Config {
    pub fn from_env() -> Self {
        fn v(k: &str, d: &str) -> String {
            std::env::var(k).unwrap_or_else(|_| d.to_string())
        }
        Config {
            endpoint: v("INTAKE_ENDPOINT", "http://localhost:9000"),
            region: v("INTAKE_REGION", "us-east-1"),
            admin_access_key: v("INTAKE_ADMIN_ACCESS_KEY", "minioadmin"),
            admin_secret_key: v("INTAKE_ADMIN_SECRET_KEY", "minioadmin"),
            mc_alias: v("INTAKE_MC_ALIAS", "myminio"),
            mc_parent: v("INTAKE_MC_PARENT", "minioadmin"),
            site_dir: v("INTAKE_SITE_DIR", "./site"),
            cases_dir: v("INTAKE_CASES_DIR", "./cases"),
        }
    }
}
