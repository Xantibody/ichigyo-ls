mod server;
mod textlint;

use tower_lsp::{LspService, Server};

use server::Backend;
use textlint::CommandRunner;

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| Backend::new(client, CommandRunner));

    Server::new(stdin, stdout, socket).serve(service).await;
}
