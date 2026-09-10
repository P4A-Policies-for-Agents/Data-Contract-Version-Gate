use serde::Deserialize;
#[derive(Deserialize, Clone, Debug)]
pub struct Contracts0Config {
    #[serde(alias = "blockedBelow")]
    pub blocked_below: Option<String>,
    #[serde(alias = "contract")]
    pub contract: String,
    #[serde(alias = "deprecatedBelow")]
    pub deprecated_below: Option<String>,
    #[serde(alias = "sunset")]
    pub sunset: Option<String>,
}
#[derive(Deserialize, Clone, Debug)]
pub struct Config {
    #[serde(alias = "applyTo")]
    pub apply_to: Option<String>,
    #[serde(alias = "blockedBelow")]
    pub blocked_below: Option<String>,
    #[serde(alias = "contractHeader")]
    pub contract_header: Option<String>,
    #[serde(alias = "contracts")]
    pub contracts: Option<Vec<Contracts0Config>>,
    #[serde(alias = "deprecatedBelow")]
    pub deprecated_below: Option<String>,
    #[serde(alias = "failMode")]
    pub fail_mode: Option<String>,
    #[serde(alias = "versionHeader")]
    pub version_header: Option<String>,
}
#[pdk::hl::entrypoint_flex]
fn init(abi: &dyn pdk::flex_abi::api::FlexAbi) -> Result<(), anyhow::Error> {
    abi.setup()?;
    Ok(())
}
