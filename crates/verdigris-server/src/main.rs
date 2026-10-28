use verdigris_protocol::{GAME_VERSION, PROTOCOL_VERSION};
use verdigris_server::{DEFAULT_BIND, serve};

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let bind = std::env::args()
        .nth(1)
        .unwrap_or_else(|| DEFAULT_BIND.to_owned());
    println!("Verdigris, Minecraft {GAME_VERSION} (protocole {PROTOCOL_VERSION}) sur {bind}");
    serve(&bind).await
}
