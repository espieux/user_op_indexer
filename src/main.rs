use ethers::prelude::*;
use ethers::contract::EthEvent;
use ethers::types::{Filter, H256};
use futures_util::StreamExt;
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use eyre::Result;
use tracing::{info, warn, error, debug};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt};

// Keep the existing UserOperationEvent struct

fn init_tracing() {
    // Get log level from environment variable with defaults
    let log_level = std::env::var("RUST_LOG")
        .unwrap_or_else(|_| "INFO".to_string())
        .to_uppercase();

    // Parse log level
    let level = match log_level.as_str() {
        "ERROR" => tracing::Level::ERROR,
        "WARN" => tracing::Level::WARN,
        "INFO" => tracing::Level::INFO,
        "DEBUG" => tracing::Level::DEBUG,
        "TRACE" => tracing::Level::TRACE,
        _ => tracing::Level::INFO, // Default to INFO
    };

    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            fmt::layer()
                .with_target(true)
                .with_thread_ids(true)
                .with_thread_names(true)
        )
        .with(tracing_subscriber::filter::LevelFilter::from(level))
        .init();

    info!("Tracing initialized at level: {}", level);
}


#[derive(Debug, Clone, EthEvent)]
#[ethevent(
    name = "UserOperationEvent",
    abi = "UserOperationEvent(bytes32 userOpHash, address sender, address paymaster, uint256 nonce, bool success, uint256 actualGasCost, uint256 actualGasUsed)"
)]
struct UserOperationEvent {
    pub user_op_hash: H256,
    pub sender: Address,
    pub paymaster: Address,
    pub nonce: U256,
    pub success: bool,
    pub actual_gas_cost: U256,
    pub actual_gas_used: U256,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing first
    init_tracing();

    // Load environment variables
    dotenv::dotenv().ok();

    // Get RPC URL
    let rpc_url = std::env::var("RPC_URL")
        .map_err(|e| {
            error!("Failed to get RPC_URL: {}", e);
            e
        })?;

    // Connect to provider
    let provider: Provider<Ws> = Provider::connect(rpc_url).await
        .map_err(|e| {
            error!("Failed to connect to provider: {}", e);
            e
        })?;
    let provider = Arc::new(provider);

    // Verify the Connection to Ethereum
    let block_number = provider.get_block_number().await
        .map_err(|e| {
            error!("Failed to get block number: {}", e);
            e
        })?;
    info!("Connected to Ethereum. Latest block: {}", block_number);

    // Entry point address
    let entry_point_address: Address = "0x0000000071727De22E5E9d8BAf0edAc6f37da032".parse()
        .map_err(|e| {
            error!("Failed to parse entry point address: {}", e);
            e
        })?;

    // Compute topic0 for verification
    let computed_topic0 = H256::from_slice(
        &ethers::utils::keccak256("UserOperationEvent(bytes32,address,address,uint256,bool,uint256,uint256)")[..],
    );
    debug!("Computed topic0: {:?}", computed_topic0);

    // Database connection
    let db_url = std::env::var("DATABASE_URL")
        .map_err(|e| {
            error!("Failed to get DATABASE_URL: {}", e);
            e
        })?;
    let db_pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await
        .map_err(|e| {
            error!("Failed to connect to database: {}", e);
            e
        })?;

    // Test database connection
    let result = sqlx::query!("SELECT 1 as value")
        .fetch_one(&db_pool)
        .await
        .map_err(|e| {
            error!("Database connection test failed: {}", e);
            e
        })?;
    info!("Database connection test successful: {:?}", result.value);

    // Get start block
    let start_block: u64 = std::env::var("START_BLOCK")
        .map_err(|e| {
            error!("Failed to get START_BLOCK: {}", e);
            e
        })?
        .parse()
        .map_err(|e| {
            error!("Failed to parse START_BLOCK: {}", e);
            e
        })?;

    // Index events
    index_events(provider, db_pool, entry_point_address, start_block).await
        .map_err(|e| {
            error!("Event indexing failed: {}", e);
            e
        })?;

    Ok(())
}

async fn index_events(
    provider: Arc<Provider<Ws>>,
    db_pool: sqlx::PgPool,
    entry_point_address: Address,
    from_block_number: u64,
) -> Result<()> {
    // Fetch latest block number
    let latest_block_number = provider.get_block_number().await?.as_u64();

    info!(
        "Fetching historical events from block {} to {}",
        from_block_number, latest_block_number
    );

    // Fetch historical logs
    let historical_logs = provider
        .get_logs(&Filter::new()
            .address(entry_point_address)
            .topic0(H256::from_slice(
                &ethers::utils::keccak256("UserOperationEvent(bytes32,address,address,uint256,bool,uint256,uint256)")[..],
            ))
            .from_block(BlockNumber::Number(from_block_number.into()))
            .to_block(BlockNumber::Number(latest_block_number.into())))
        .await
        .map_err(|e| {
            error!("Failed to fetch historical logs: {}", e);
            e
        })?;

    // Process historical logs
    for log in historical_logs {
        match decode_user_operation_event(&log) {
            Ok((event, block_number)) => {
                debug!("Historical event: {:?}", event);
                save_event_to_db(&db_pool, event, block_number).await
                    .map_err(|e| {
                        warn!("Failed to save historical event: {}", e);
                        e
                    })?;
            }
            Err(e) => {
                warn!("Error decoding historical event: {:?}", e);
            }
        }
    }

    // Listen for new events
    info!("Listening for new events from block {}", latest_block_number + 1);

    let filter = Filter::new()
        .address(entry_point_address)
        .topic0(H256::from_slice(
            &ethers::utils::keccak256("UserOperationEvent(bytes32,address,address,uint256,bool,uint256,uint256)")[..],
        ))
        .from_block(BlockNumber::Number((latest_block_number + 1).into()));

    let mut stream = provider.subscribe_logs(&filter)
        .await
        .map_err(|e| {
            error!("Failed to subscribe to logs: {}", e);
            e
        })?;

    while let Some(log) = stream.next().await {
        match decode_user_operation_event(&log) {
            Ok((event, block_number)) => {
                debug!("New event: {:?}", event);
                save_event_to_db(&db_pool, event, block_number).await
                    .map_err(|e| {
                        warn!("Failed to save new event: {}", e);
                        e
                    })?;
            }
            Err(e) => {
                warn!("Error decoding new event: {:?}", e);
            }
        }
    }

    Ok(())
}

fn decode_user_operation_event(log: &Log) -> Result<(UserOperationEvent, u64), ethers::abi::Error> {
    // Ensure the log contains the expected topics
    if log.topics.len() != 4 {
        warn!(
            "Invalid number of topics: expected 4, got {}",
            log.topics.len()
        );
        return Err(ethers::abi::Error::InvalidData);
    }

    // Decode indexed fields from topics
    let user_op_hash = H256::from(log.topics[1]); // topic[1]: bytes32
    let sender: Address = log.topics[2].into();  // topic[2]: address
    let paymaster: Address = log.topics[3].into(); // topic[3]: address

    // Decode non-indexed fields from data
    let data = &log.data.0;

    if data.len() != 128 {
        warn!(
            "Unexpected data length: expected 128, got {}",
            data.len()
        );
        return Err(ethers::abi::Error::InvalidData);
    }

    // Decode fields accounting for padding
    let nonce = U256::from_big_endian(&data[0..32]);          // uint256
    let success = data[63] != 0;                              // bool (1 byte after 31 bytes of padding)
    let actual_gas_cost = U256::from_big_endian(&data[64..96]); // uint256
    let actual_gas_used = U256::from_big_endian(&data[96..128]); // uint256

    // Get the block number from the log
    let block_number = log
        .block_number
        .ok_or(ethers::abi::Error::InvalidData)
        .map_err(|e| {
            warn!("Failed to extract block number from log");
            e
        })?
        .as_u64();

    debug!(
        "Decoded UserOperationEvent: hash={:?}, sender={:?}, paymaster={:?}, nonce={}, success={}, gas_cost={}, gas_used={}, block={}",
        user_op_hash, sender, paymaster, nonce, success, actual_gas_cost, actual_gas_used, block_number
    );

    // Construct the UserOperationEvent struct
    Ok((
        UserOperationEvent {
            user_op_hash,
            sender,
            paymaster,
            nonce,
            success,
            actual_gas_cost,
            actual_gas_used,
        },
        block_number,
    ))
}

async fn save_event_to_db(
    db_pool: &sqlx::PgPool,
    event: UserOperationEvent,
    block_number: u64,
) -> Result<()> {
    let user_op_hash = format!("{:?}", event.user_op_hash);
    let sender = format!("{:?}", event.sender);
    let paymaster = format!("{:?}", event.paymaster);
    let nonce = format!("0x{:064x}", event.nonce); // Ensure consistent formatting

    match sqlx::query!(
        r#"
        INSERT INTO user_operation_events (user_op_hash, sender, paymaster, nonce, success, actual_gas_cost, actual_gas_used, block_number)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        ON CONFLICT (user_op_hash, nonce) DO NOTHING
        "#,
        user_op_hash,
        sender,
        paymaster,
        nonce,
        event.success,
        event.actual_gas_cost.as_u128() as i64,
        event.actual_gas_used.as_u128() as i64,
        block_number as i64
    )
    .execute(db_pool)
    .await
    {
        Ok(result) => {
            if result.rows_affected() > 0 {
                info!(
                    "Event saved to database at block {} (user_op_hash: {})",
                    block_number, user_op_hash
                );
            } else {
                debug!(
                    "Duplicate event skipped at block {} (user_op_hash: {})",
                    block_number, user_op_hash
                );
            }
            Ok(())
        }
        Err(e) => {
            error!(
                "Failed to save event to database (user_op_hash: {}): {}",
                user_op_hash, e
            );
            Err(e.into())
        }
    }
}