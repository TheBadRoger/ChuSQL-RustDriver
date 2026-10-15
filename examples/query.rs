use chusql_driver::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let address = std::env::var("CHUSQL_ADDRESS").unwrap_or_else(|_| "127.0.0.1:7777".into());
    let user = std::env::var("CHUSQL_USER")?;
    let password = std::env::var("CHUSQL_PASSWORD")?;
    let sql = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "SHOW DATABASES".into());
    let client = Client::connect(address, &user, &password).await?;
    let result = client.query(&sql).await?;
    println!("{:?}", result);
    client.close().await;
    Ok(())
}
