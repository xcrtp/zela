//! `zela` 可执行入口：宿主实现全在 `zela_cli`（lib），这里只做退出码映射。

fn main() {
    match zela_cli::dispatch(std::env::args().collect()) {
        Ok(response) => {
            let text = zela_cli::format_human(&response);
            if !text.is_empty() {
                println!("{text}");
            }
        }
        Err(message) => {
            eprintln!("zela: {message}");
            std::process::exit(1);
        }
    }
}
