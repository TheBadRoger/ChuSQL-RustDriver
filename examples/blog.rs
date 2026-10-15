//! Run against a disposable database: creates and deletes a demonstration post.
use chusql_driver::{Client, Query};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let address = std::env::var("CHUSQL_ADDRESS").unwrap_or_else(|_| "127.0.0.1:7777".into());
    let db = Client::connect(
        address,
        &std::env::var("CHUSQL_USER")?,
        &std::env::var("CHUSQL_PASSWORD")?,
    )
    .await?;
    // Use a dedicated session so USE and subsequent operations cannot interleave with other handlers.
    let database = std::env::var("CHUSQL_DATABASE")?;
    if database.is_empty()
        || !database
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
        || database.starts_with(|c: char| c.is_ascii_digit())
    {
        return Err("CHUSQL_DATABASE must be a plain identifier".into());
    }
    db.query(&format!("USE {database}")).await?;
    // Run once in a disposable database; table must not already exist.
    db.query("CREATE TABLE driver_blog_posts (id INT PRIMARY KEY, title TEXT, body TEXT, published BOOL)").await?;
    let insert = Query::new("INSERT INTO driver_blog_posts (id, title, body, published) VALUES (")
        .bind(1_i64)
        .push(", ")
        .bind("ChuSQL 博客")
        .push(", ")
        .bind("第一篇文章：It's simple.\n正文支持换行。")
        .push(", ")
        .bind(true)
        .push(")");
    db.query(insert.as_str()).await?;
    let update = Query::new("UPDATE driver_blog_posts SET title = ")
        .bind("更新后的标题")
        .push(" WHERE id = ")
        .bind(1_i64);
    db.query(update.as_str()).await?;
    let select = Query::new("SELECT id, title, body FROM driver_blog_posts WHERE published = ")
        .bind(true)
        .push(" ORDER BY id DESC LIMIT 10");
    let result = db.query(select.as_str()).await?;
    assert!(!result.truncated);
    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0][1].as_str(), Some("更新后的标题"));
    println!("{:?}", result.rows);
    let delete = Query::new("DELETE FROM driver_blog_posts WHERE id = ").bind(1_i64);
    db.query(delete.as_str()).await?;
    db.query("DROP TABLE driver_blog_posts").await?;
    db.close().await;
    Ok(())
}
