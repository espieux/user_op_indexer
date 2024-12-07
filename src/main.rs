use ethers::prelude::*;
use ethers::contract::EthEvent;
use ethers::types::{Filter, H256};
use futures_util::StreamExt;
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use eyre::Result;

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
    dotenv::dotenv().ok();
    let rpc_url = std::env::var("RPC_URL")?;
    let provider: Provider<Ws> = Provider::connect(rpc_url).await?;
    let provider = Arc::new(provider);

    // Verify the Connection to Ethereum
    let block_number = provider.get_block_number().await?;
    println!("Connected to Ethereum. Latest block: {:?}", block_number);

    let entry_point_address: Address = "0x0000000071727De22E5E9d8BAf0edAc6f37da032".parse()?;

    // Verify the Contract Connection
    let computed_topic0 = H256::from_slice(
        &ethers::utils::keccak256("UserOperationEvent(bytes32,address,address,uint256,bool,uint256,uint256)")[..],
    );
    println!("Computed topic0: {:?}", computed_topic0);

    let db_url = std::env::var("DATABASE_URL")?;
    let db_pool = PgPoolOptions::new().max_connections(5).connect(&db_url).await?;

    // Check the Database Connection
    let result = sqlx::query!("SELECT 1 as value").fetch_one(&db_pool).await?;
    println!("Test query result: {:?}", result.value);

    let start_block: u64 = std::env::var("START_BLOCK")?.parse()?;

    index_events(provider, db_pool, entry_point_address, start_block).await?;
    Ok(())
}

async fn index_events(
    provider: Arc<Provider<Ws>>,
    db_pool: sqlx::PgPool,
    entry_point_address: Address,
    from_block_number: u64,
) -> Result<()> {
    // Step 1: Fetch Historical Events
    let latest_block_number = provider.get_block_number().await?.as_u64();

    println!(
        "Fetching historical events from block {} to {}",
        from_block_number, latest_block_number
    );

    let historical_logs = provider
        .get_logs(&Filter::new()
            .address(entry_point_address)
            .topic0(H256::from_slice(
                &ethers::utils::keccak256("UserOperationEvent(bytes32,address,address,uint256,bool,uint256,uint256)")[..],
            ))
            .from_block(BlockNumber::Number(from_block_number.into()))
            .to_block(BlockNumber::Number(latest_block_number.into())))
        .await?;

    for log in historical_logs {
        // println!("Raw log received: {:?}", log);
        match decode_user_operation_event(&log) {
            Ok((event,block_number)) => {
                println!("Historical event: {:?}", event);
                save_event_to_db(&db_pool, event, block_number).await?;
            }
            Err(e) => {
                eprintln!("Error decoding historical event: {:?}", e);
            }
        }
    }

    // Step 2: Listen for New Events
    println!("Listening for new events from block {}", latest_block_number + 1);

    let filter = Filter::new()
        .address(entry_point_address)
        .topic0(H256::from_slice(
            &ethers::utils::keccak256("UserOperationEvent(bytes32,address,address,uint256,bool,uint256,uint256)")[..],
        ))
        .from_block(BlockNumber::Number((latest_block_number + 1).into()));

    let mut stream = provider.subscribe_logs(&filter).await?;

    while let Some(log) = stream.next().await {
        // println!("Raw log received: {:?}", log);
        match decode_user_operation_event(&log) {
            Ok((event,block_number)) => {
                println!("New event: {:?}", event);
                save_event_to_db(&db_pool, event, block_number).await?;
            }
            Err(e) => {
                eprintln!("Error decoding new event: {:?}", e);
            }
        }
    }

    Ok(())
}


fn decode_user_operation_event(log: &Log) -> Result<(UserOperationEvent, u64), ethers::abi::Error> {
    // Ensure the log contains the expected topics
    if log.topics.len() != 4 {
        println!("Invalid number of topics: {:?}", log.topics.len());
        return Err(ethers::abi::Error::InvalidData);
    }

    // Decode indexed fields from `topics`
    let user_op_hash = H256::from(log.topics[1]); // topic[1]: bytes32
    let sender: Address = log.topics[2].into();  // topic[2]: address
    let paymaster: Address = log.topics[3].into(); // topic[3]: address

    // Decode non-indexed fields from `data`
    let data = &log.data.0;

    if data.len() != 128 {
        println!("Unexpected data length: {:?}", data.len());
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
        .ok_or(ethers::abi::Error::InvalidData)?
        .as_u64();

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
    block_number: u64, // Pass block number as an argument
) -> Result<()> {
    let user_op_hash = format!("{:?}", event.user_op_hash);
    let sender = format!("{:?}", event.sender);
    let paymaster = format!("{:?}", event.paymaster);
    let nonce = format!("0x{:064x}", event.nonce); // Ensure consistent formatting

    sqlx::query!(
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
        block_number as i64 // Insert block number
    )
    .execute(db_pool)
    .await?;

    println!(
        "Event saved to database (or skipped if duplicate) at block {}",
        block_number
    );
    Ok(())
}