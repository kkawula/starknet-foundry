use super::declare_deploy::DeclareDeploy;
use anyhow::{anyhow, Result};
use clap::Args;
use sncast::helpers::deploy::{DeployArgs, DeployVersion};
use sncast::helpers::error::token_not_supported_for_deployment;
use sncast::helpers::fee::{FeeArgs, FeeSettings, FeeToken, PayableTransaction};
use sncast::helpers::rpc::RpcArgs;
use sncast::helpers::scarb_utils::{read_manifest_and_build_artifacts, CompiledContract};
use sncast::response::errors::StarknetCommandError;
use sncast::response::structs::{DeployResponse, Felt};
use sncast::{extract_or_generate_salt, impl_payable_transaction, udc_uniqueness};
use sncast::{handle_wait_for_tx, WaitForTx};
use starknet::accounts::AccountError::Provider;
use starknet::accounts::{Account, ConnectedAccount, SingleOwnerAccount};
use starknet::contract::ContractFactory;
use starknet::core::types::FieldElement;
use starknet::core::utils::get_udc_deployed_address;
use starknet::providers::jsonrpc::HttpTransport;
use starknet::providers::JsonRpcClient;
use starknet::signers::LocalWallet;

#[derive(Args)]
#[command(about = "Deploy a contract on Starknet")]
pub struct Deploy {
    /// Class hash of contract to deploy
    #[clap(short = 'g', long, conflicts_with = "contract_name")]
    pub class_hash: Option<FieldElement>,

    // Name of the contract to deploy
    #[clap(long, conflicts_with = "class_hash")]
    pub contract_name: Option<String>,

    #[clap(flatten)]
    pub args: DeployArgs,

    #[clap(flatten)]
    pub fee_args: FeeArgs,

    #[clap(flatten)]
    pub rpc: RpcArgs,
}

impl From<DeclareDeploy> for Deploy {
    fn from(declare_deploy: DeclareDeploy) -> Self {
        let DeclareDeploy {
            contract_name,
            deploy_args,
            fee_token,
            rpc,
        } = declare_deploy;

        let fee_args = FeeArgs {
            fee_token: Some(fee_token),
            ..Default::default()
        };

        Deploy {
            class_hash: None,
            contract_name: Some(contract_name),
            args: deploy_args,
            fee_args,
            rpc,
        }
    }
}

impl Deploy {
    pub fn build_artifacts_and_get_compiled_contract(
        &self,
        json: bool,
        profile: &Option<String>,
    ) -> Result<CompiledContract> {
        let contract_name = self
            .contract_name
            .clone()
            .ok_or_else(|| anyhow!("Contract name and class hash unspecified"))?;

        let artifacts = read_manifest_and_build_artifacts(&self.args.package, json, profile)?;

        let contract_artifacts = artifacts
            .get(&contract_name)
            .ok_or_else(|| anyhow!("No artifacts found for contract: {}", contract_name))?;

        contract_artifacts.try_into()
    }

    pub fn resolved_with_class_hash(mut self, value: FieldElement) -> DeployResolved {
        self.class_hash = Some(value);
        self.into()
    }
}

pub struct DeployResolved {
    pub class_hash: FieldElement,
    pub constructor_calldata: Vec<FieldElement>,
    pub salt: Option<FieldElement>,
    pub unique: bool,
    pub fee_args: FeeArgs,
    pub nonce: Option<FieldElement>,
    pub version: Option<DeployVersion>,
}

impl From<Deploy> for DeployResolved {
    fn from(deploy: Deploy) -> Self {
        let Deploy {
            class_hash,
            args:
                DeployArgs {
                    constructor_calldata,
                    salt,
                    unique,
                    nonce,
                    version,
                    ..
                },
            fee_args,
            ..
        } = deploy;

        DeployResolved {
            class_hash: class_hash.unwrap(),
            constructor_calldata,
            salt,
            unique,
            fee_args,
            nonce,
            version,
        }
    }
}

impl_payable_transaction!(DeployResolved, token_not_supported_for_deployment,
    DeployVersion::V1 => FeeToken::Eth,
    DeployVersion::V3 => FeeToken::Strk
);

pub async fn deploy(
    deploy: DeployResolved,
    account: &SingleOwnerAccount<&JsonRpcClient<HttpTransport>, LocalWallet>,
    wait_config: WaitForTx,
) -> Result<DeployResponse, StarknetCommandError> {
    let fee_settings = deploy
        .fee_args
        .clone()
        .fee_token(deploy.token_from_version())
        .try_into_fee_settings(account.provider(), account.block_id())
        .await?;

    let salt = extract_or_generate_salt(deploy.salt);
    let factory = ContractFactory::new(deploy.class_hash, account);
    let result = match fee_settings {
        FeeSettings::Eth { max_fee } => {
            let execution =
                factory.deploy_v1(deploy.constructor_calldata.clone(), salt, deploy.unique);
            let execution = match max_fee {
                None => execution,
                Some(max_fee) => execution.max_fee(max_fee),
            };
            let execution = match deploy.nonce {
                None => execution,
                Some(nonce) => execution.nonce(nonce),
            };
            execution.send().await
        }
        FeeSettings::Strk {
            max_gas,
            max_gas_unit_price,
        } => {
            let execution =
                factory.deploy_v3(deploy.constructor_calldata.clone(), salt, deploy.unique);

            let execution = match max_gas {
                None => execution,
                Some(max_gas) => execution.gas(max_gas),
            };
            let execution = match max_gas_unit_price {
                None => execution,
                Some(max_gas_unit_price) => execution.gas_price(max_gas_unit_price),
            };
            let execution = match deploy.nonce {
                None => execution,
                Some(nonce) => execution.nonce(nonce),
            };
            execution.send().await
        }
    };

    match result {
        Ok(result) => handle_wait_for_tx(
            account.provider(),
            result.transaction_hash,
            DeployResponse {
                contract_address: Felt(get_udc_deployed_address(
                    salt,
                    deploy.class_hash,
                    &udc_uniqueness(deploy.unique, account.address()),
                    &deploy.constructor_calldata,
                )),
                transaction_hash: Felt(result.transaction_hash),
            },
            wait_config,
        )
        .await
        .map_err(StarknetCommandError::from),
        Err(Provider(error)) => Err(StarknetCommandError::ProviderError(error.into())),
        _ => Err(anyhow!("Unknown RPC error").into()),
    }
}
