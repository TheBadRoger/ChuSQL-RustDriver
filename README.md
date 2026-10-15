# ChuSQL Rust Driver

轻量异步 ChuSQL 数据库驱动，使用 Tokio，通过 TCP 直接连接 `chusql-server`。可作为 Axum、Actix Web 等 Tokio 后端框架的应用状态使用。

实现依据是相邻 ChuSQL 项目的 `chusql-interface/src/ChuSQL/Interface/Protocol.hs`、`Link.hs` 和 `chusql-core/model/src/ChuSQL/Core/Protocol.hs`，使用版本 1 的 UTF-8 JSON 行协议。依赖仅有 Tokio、Serde 和 serde_json，没有 ORM 或连接池层。

## 接入

在后端项目的 `Cargo.toml` 中添加：

```toml
[dependencies]
chusql-driver = { path = "../ChuSQLDriver" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

```rust,no_run
use chusql_driver::{Client, Error};

#[tokio::main]
async fn main() -> Result<(), Error> {
    let db = Client::connect("127.0.0.1:7777", "app", "your-password").await?;
    db.ping().await?;
    let result = db.query("SHOW DATABASES").await?;
    println!("{:?}", result.columns);
    for row in &result.rows {
        println!("{row:?}");
    }
    if result.truncated {
        println!("结果已被服务端截断，请缩小查询范围");
    }
    db.close().await;
    Ok(())
}
```

需要已初始化并启动 ChuSQL TCP 服务，且登录账号具备相应 SQL 权限。默认地址为 `127.0.0.1:7777`。

`Client` 可 `clone()`，支持 `Send + Sync`。将它传给 Axum 的 `Router::with_state(db)` 或 Actix Web 的 `web::Data::new(db)`，handler 中直接调用 `db.query(sql).await`。所有克隆共享同一个登录会话，同一时刻只执行一个请求；需要并行执行时建立多个独立连接。不要在多个 handler 间交错执行依赖会话状态的一组 SQL（例如切换数据库或事务），这类操作应使用独立连接。

## API 与结果

- `Client::connect(address, user, password)`：连接、协议握手、账号登录。
- `Client::connect_with_options(address, user, password, options)`：自定义连接/请求超时和响应字节上限。
- `client.query(sql)`：执行单条 SQL，DDL、DML 和 SELECT 使用相同入口。
- `client.ping()`：探活。
- `client.close()`：关闭所有克隆共享的连接；也可在最后一个客户端释放时自动断开。

`QueryResult` 保留服务端的 `columns`、`rows`、`row_count`、`truncated` 和 `database`。行内值使用 `serde_json::Value`，可以使用 `as_str()`、`as_i64()`、`as_bool()`、`is_null()` 等方法读取。数值启用任意精度 JSON 表示，运行时自定义类型对象原样保留。`row_count` 是服务端返回的行数，不承诺等同于写操作影响行数；务必检查 `truncated`。

```rust
use chusql_driver::Options;
use std::time::Duration;

let options = Options {
    timeout: Duration::from_secs(5),
    max_response_bytes: 8 * 1024 * 1024,
};
```

默认连接及每次请求超时为 30 秒，单条响应上限为 16 MiB（含换行）；等待其他请求释放连接的时间不计入超时。可匹配 `Error::Server { code, message }` 处理服务端错误，普通 SQL 错误不会关闭连接。I/O、JSON、超时或取消会使连接失效，后续返回 `Error::Closed`，需要重新建立连接。驱动不自动重试写入：断线或超时发生时，SQL 可能已执行。

## 边界

当前 ChuSQL 协议只接收 SQL 文本，不提供服务端参数绑定。新增的 `Query` 按 ChuSQL 解析规则编码客户端字面量；不要将用户输入直接拼接到 SQL，也不要将其传给 `Query::new` 或 `push`。

```rust
use chusql_driver::Query;

let title = "It's my blog";
let statement = Query::new("INSERT INTO posts (id, title) VALUES (")
    .bind(1_i64)
    .push(", ")
    .bind(title)
    .push(")");
// db.query(statement.as_str()).await?;
```

`bind` 支持 `&str`、`i64`、`bool` 和 `Param::Null`，仅用于 SQL 值。表名和列名必须由代码固定或使用白名单；LIKE 通配符仍有其原本语义。这不是服务端预编译语句，也不模拟问号占位符。

## 轻量博客

当前驱动适合低并发个人博客的文章、评论和分类读写。新增 `examples/blog.rs` 展示建表、含单引号/中文/换行的正文写入、更新、列表和删除。对一个空的测试数据库设置 `CHUSQL_DATABASE` 后运行 `cargo run --example blog`；示例会创建并最终删除 `driver_blog_posts`，中途失败时可能保留表。数据库需要事先创建，账号需要建表和读写权限。

查询可使用 `ORDER BY id DESC LIMIT 10`，翻页使用 `WHERE id < 上一页最后的ID`，通过 `bind` 传入 ID；当前解析器没有 OFFSET。大正文应按需查询，避免列表加载全部正文。ChuSQL 默认请求上限为 1 MiB、结果行数上限为 1000，图片建议独立存储。

一个共享连接可以作为低流量起点；事务和 `USE` 需要独立连接。超时后重新连接，但不要盲目重试发文；使用唯一 ID 或唯一 slug 避免重复提交。生产使用前仍需对真实 ChuSQL 验证权限、备份恢复和预期并发，驱动本身不保证数据库的成熟度。

服务器协议通过明文 TCP 传输账号密码及查询数据，跨不可信网络时请使用受保护的网络或加密隧道。本驱动不内置 TLS、ORM 适配、连接池、迁移或自动重连，先保持核心连接功能简单。

## 验证与示例

```sh
cargo test --all-targets
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

协议测试使用本地 TCP 测试服务覆盖握手、登录、Unicode/换行、结果映射、错误恢复、响应分片、损坏/超长/未终止响应、超时和请求取消。它们不替代真实数据库联调。

对运行中的真实数据库执行查询：

```sh
export CHUSQL_ADDRESS=127.0.0.1:7777
export CHUSQL_USER=app
# 在运行环境中设置 CHUSQL_PASSWORD
cargo run --example query -- 'SHOW DATABASES'
```
