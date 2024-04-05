use std::{sync::Arc, vec};

use ethers::{
    abi::{ParamType, Token},
    providers::Middleware,
    types::{Bytes, I256},
};
use tracing::instrument;

use crate::{
    amm::{AutomatedMarketMaker, AMM},
    errors::AMMError,
};

use super::UniswapV3PoolCustomized;

use ethers::prelude::abigen;

abigen!(
    IGetUniswapV3PoolDataBatchRequest,
    "src/amm/uniswap_v3_customized/batch_request/GetUniswapV3PoolDataBatchRequestABI.json"
);

fn populate_pool_data_from_tokens(
    mut pool: UniswapV3PoolCustomized,
    tokens: Vec<Token>,
) -> Option<UniswapV3PoolCustomized> {
    pool.token_a = tokens[0].to_owned().into_address()?;
    pool.token_a_decimals = tokens[1].to_owned().into_uint()?.as_u32() as u8;
    pool.token_b = tokens[2].to_owned().into_address()?;
    pool.token_b_decimals = tokens[3].to_owned().into_uint()?.as_u32() as u8;
    pool.liquidity = tokens[4].to_owned().into_uint()?.as_u128();
    pool.sqrt_price = tokens[5].to_owned().into_uint()?;
    pool.tick = I256::from_raw(tokens[6].to_owned().into_int()?).as_i32();
    pool.tick_spacing = I256::from_raw(tokens[7].to_owned().into_int()?).as_i32();
    pool.fee = tokens[8].to_owned().into_uint()?.as_u64() as u32;

    Some(pool)
}

pub async fn get_v3_pool_data_batch_request<M: Middleware>(
    pool: &mut UniswapV3PoolCustomized,
    block_number: Option<u64>,
    middleware: Arc<M>,
) -> Result<(), AMMError<M>> {
    let constructor_args = Token::Tuple(vec![Token::Array(vec![Token::Address(pool.address)])]);

    let deployer = IGetUniswapV3PoolDataBatchRequest::deploy(middleware.clone(), constructor_args)?;

    let return_data: Bytes = if let Some(block_number) = block_number {
        deployer.block(block_number).call_raw().await?
    } else {
        deployer.call_raw().await?
    };

    let return_data_tokens = ethers::abi::decode(
        &[ParamType::Array(Box::new(ParamType::Tuple(vec![
            ParamType::Address,   // token a
            ParamType::Uint(8),   // token a decimals
            ParamType::Address,   // token b
            ParamType::Uint(8),   // token b decimals
            ParamType::Uint(128), // liquidity
            ParamType::Uint(160), // sqrtPrice
            ParamType::Int(24),   // tick
            ParamType::Int(24),   // tickSpacing
            ParamType::Uint(24),  // fee
            ParamType::Int(128),  // liquidityNet
        ])))],
        &return_data,
    )?;

    //Update pool data
    for tokens in return_data_tokens {
        if let Some(tokens_arr) = tokens.into_array() {
            for tup in tokens_arr {
                let pool_data = tup
                    .into_tuple()
                    .ok_or(AMMError::BatchRequestError(pool.address))?;

                *pool = populate_pool_data_from_tokens(pool.to_owned(), pool_data)
                    .ok_or(AMMError::BatchRequestError(pool.address))?;
            }
        }
    }
    Ok(())
}

pub struct UniswapV3TickData {
    pub initialized: bool,
    pub tick: i32,
    pub liquidity_net: i128,
}

#[instrument(skip(middleware) level = "debug")]
pub async fn get_amm_data_batch_request<M: Middleware>(
    amms: &mut [AMM],
    block_number: u64,
    middleware: Arc<M>,
) -> Result<(), AMMError<M>> {
    let mut target_addresses = vec![];

    for amm in amms.iter() {
        target_addresses.push(Token::Address(amm.address()));
    }

    let constructor_args = Token::Tuple(vec![Token::Array(target_addresses)]);
    let deployer = IGetUniswapV3PoolDataBatchRequest::deploy(middleware.clone(), constructor_args)?;

    let return_data: Bytes = deployer.block(block_number).call_raw().await?;

    let return_data_tokens = ethers::abi::decode(
        &[ParamType::Array(Box::new(ParamType::Tuple(vec![
            ParamType::Address,   // token a
            ParamType::Uint(8),   // token a decimals
            ParamType::Address,   // token b
            ParamType::Uint(8),   // token b decimals
            ParamType::Uint(128), // liquidity
            ParamType::Uint(160), // sqrtPrice
            ParamType::Int(24),   // tick
            ParamType::Int(24),   // tickSpacing
            ParamType::Uint(24),  // fee
            ParamType::Int(128),  // liquidityNet
        ])))],
        &return_data,
    )?;

    let mut pool_idx = 0;

    //Update pool data
    for tokens in return_data_tokens {
        if let Some(tokens_arr) = tokens.into_array() {
            for tup in tokens_arr {
                if let Some(pool_data) = tup.into_tuple() {
                    if let Some(address) = pool_data[0].to_owned().into_address() {
                        if !address.is_zero() {
                            //Update the pool data
                            if let AMM::UniswapV3PoolCustomized(uniswap_v3_pool) = amms
                                .get_mut(pool_idx)
                                .expect("Pool idx should be in bounds")
                            {
                                if let Some(pool) = populate_pool_data_from_tokens(
                                    uniswap_v3_pool.to_owned(),
                                    pool_data,
                                ) {
                                    tracing::trace!(?pool);
                                    *uniswap_v3_pool = pool;
                                }
                            }
                        }
                    }
                    pool_idx += 1;
                }
            }
        }
    }

    //TODO: should we clean up empty pools here?

    Ok(())
}
