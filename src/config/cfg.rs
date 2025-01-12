use config::Config as _Config;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;
use tendermint_rpc::error::ErrorDetail::UnsupportedScheme;
use tendermint_rpc::{Error, Url};

#[cfg(feature = "chain-reg")]
use crate::orchestrator::cosm_orc::tokio_block;
#[cfg(feature = "chain-reg")]
use rand::Rng;

use super::error::ConfigError;
use crate::{client::error::ClientError, orchestrator::deploy::DeployInfo};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub chain_cfg: ChainCfg,
    // used to configure already stored contract code_id and deployed addresses
    #[serde(default)]
    pub contract_deploy_info: HashMap<String, DeployInfo>,
}

impl Config {
    /// Reads a yaml file containing a `ConfigInput` and converts it to a useable `Config` object.
    pub fn from_yaml(file: &str) -> Result<Config, ConfigError> {
        let settings = _Config::builder()
            .add_source(config::File::with_name(file))
            .build()?;
        let cfg = settings.try_deserialize::<ConfigInput>()?;

        let contract_deploy_info = cfg.contract_deploy_info.clone();
        let chain_cfg = cfg.to_chain_cfg()?;

        Ok(Config {
            chain_cfg,
            contract_deploy_info,
        })
    }

    pub fn from_config_input(cfg_input: ConfigInput) -> Result<Self, ConfigError> {
        Ok(Self {
            contract_deploy_info: cfg_input.contract_deploy_info.clone(),
            chain_cfg: cfg_input.to_chain_cfg()?,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChainCfg {
    pub denom: String,
    pub prefix: String,
    pub chain_id: String,
    pub rpc_endpoint: String,
    pub grpc_endpoint: String,
    pub gas_prices: f64,
    pub gas_adjustment: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConfigInput {
    pub chain_cfg: ChainConfig,
    #[serde(default)]
    pub contract_deploy_info: HashMap<String, DeployInfo>,
}

impl ConfigInput {
    pub fn to_chain_cfg(self) -> Result<ChainCfg, ConfigError> {
        let chain_cfg = match self.chain_cfg {
            ChainConfig::Custom(chain_cfg) => {
                // parse and optionally fix scheme for configured api endpoints:
                let rpc_endpoint = parse_url(&chain_cfg.rpc_endpoint)?;
                let grpc_endpoint = parse_url(&chain_cfg.grpc_endpoint)?;

                ChainCfg {
                    denom: chain_cfg.denom,
                    prefix: chain_cfg.prefix,
                    chain_id: chain_cfg.chain_id,
                    gas_prices: chain_cfg.gas_prices,
                    gas_adjustment: chain_cfg.gas_adjustment,
                    rpc_endpoint,
                    grpc_endpoint,
                }
            }

            #[cfg(feature = "chain-reg")]
            ChainConfig::ChainRegistry(chain_id) => {
                // get ChainCfg from Chain Registry API:
                let chain = tokio_block(chain_registry::get::get_chain(&chain_id))
                    .map_err(|e| ConfigError::ChainRegistryAPI { source: e })?
                    .ok_or_else(|| ConfigError::ChainID {
                        chain_id: chain_id.clone(),
                    })?;

                let fee_token =
                    chain
                        .fees
                        .fee_tokens
                        .get(0)
                        .ok_or_else(|| ConfigError::MissingFee {
                            chain_id: chain_id.clone(),
                        })?;

                let mut rng = rand::thread_rng();

                let mut rpc_endpoint = chain
                    .apis
                    .rpc
                    .get(rng.gen_range(0..chain.apis.rpc.len()))
                    .ok_or_else(|| ConfigError::MissingRPC {
                        chain_id: chain_id.clone(),
                    })?
                    .address
                    .clone();

                let mut grpc_endpoint = chain
                    .apis
                    .grpc
                    .get(rng.gen_range(0..chain.apis.grpc.len()))
                    .ok_or_else(|| ConfigError::MissingGRPC {
                        chain_id: chain_id.clone(),
                    })?
                    .address
                    .clone();

                // parse and optionally fix scheme for configured api endpoints:
                rpc_endpoint = parse_url(&rpc_endpoint)?;
                grpc_endpoint = parse_url(&grpc_endpoint)?;

                ChainCfg {
                    denom: fee_token.denom.clone(),
                    prefix: chain.bech32_prefix,
                    chain_id: chain.chain_id,
                    gas_prices: fee_token.average_gas_price.into(),
                    // TODO: We should probably let the user configure `gas_adjustment` for this path as well
                    gas_adjustment: 1.5,
                    rpc_endpoint,
                    grpc_endpoint,
                }
            }
        };

        Ok(chain_cfg)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ChainConfig {
    /// Allows you to manually configure any cosmos based chain
    Custom(ChainCfg),
    /// Uses the cosmos chain registry to auto-populate ChainCfg based on given chain_id string
    /// Enable `chain-reg` feature to use.
    #[cfg(feature = "chain-reg")]
    ChainRegistry(String),
}

// Attempt to parse the configured url to ensure that it is valid.
// If url is missing the Scheme then default to https.
fn parse_url(url: &str) -> Result<String, Error> {
    let u = Url::from_str(url);

    if let Err(Error(UnsupportedScheme(detail), report)) = u {
        // if url is missing the scheme, then we will default to https:
        if !url.contains("://") {
            return Ok(format!("https://{}", url));
        }

        return Err(Error(UnsupportedScheme(detail), report));
    }

    Ok(u?.to_string())
}

#[derive(Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub struct Coin {
    pub denom: String,
    pub amount: u64,
}

impl TryFrom<Coin> for cosmrs::Coin {
    type Error = ClientError;

    fn try_from(value: Coin) -> Result<Self, ClientError> {
        Ok(Self {
            denom: value.denom.parse().map_err(|_| ClientError::Denom {
                name: value.denom.clone(),
            })?,
            amount: value.amount.into(),
        })
    }
}
