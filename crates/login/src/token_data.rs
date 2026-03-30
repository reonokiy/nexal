use serde::Deserialize;
use serde::Serialize;

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
pub struct TokenData {
    pub id_token: IdTokenInfo,
    pub access_token: String,
    pub refresh_token: String,
    pub account_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct IdTokenInfo {
    pub email: Option<String>,
    pub chatgpt_plan_type: Option<PlanType>,
    pub chatgpt_user_id: Option<String>,
    pub chatgpt_account_id: Option<String>,
    #[serde(default)]
    pub raw_jwt: String,
}

impl IdTokenInfo {
    pub fn is_workspace_account(&self) -> bool {
        matches!(
            self.chatgpt_plan_type,
            Some(PlanType::Known(KnownPlan::Team | KnownPlan::Business | KnownPlan::Enterprise | KnownPlan::Edu))
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PlanType {
    Known(KnownPlan),
    Unknown(String),
}


#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KnownPlan {
    Free, Go, Plus, Pro, Team, Business,
    #[serde(alias = "hc")]
    Enterprise,
    Edu,
}

