use anyhow::{bail, Context, Result};
use simply_nlp_labeling::{app, build_state, legacy, Config};
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

const USAGE: &str = "用法：
  simply-nlp-labeling                                      啟動伺服器
  simply-nlp-labeling import-legacy <emoLabel.db> [選項]   把舊版的資料庫匯入成新專案
      --name <專案名稱>        預設「舊版資料（檔名）」
      --utc-offset <+08:00>   舊版伺服器的時區，預設是這台電腦的時區

設定用環境變數，見 README。";

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn")))
        .init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let config = Config::from_env()?;
    match args.first().map(String::as_str) {
        None => serve(config).await,
        Some("import-legacy") => import_legacy(config, &args[1..]).await,
        Some("-h" | "--help" | "help") => {
            println!("{USAGE}");
            Ok(())
        }
        Some(other) => bail!("不認得的指令：{other}\n\n{USAGE}"),
    }
}

async fn serve(config: Config) -> Result<()> {
    match &config.llm.provider {
        Some(p) => tracing::info!("AI 輔助標注：{}（{}）", p.name(), p.model()),
        None => tracing::info!(
            "AI 輔助標注沒有開啟：{}",
            config.llm.disabled_reason.as_deref().unwrap_or("沒有設定")
        ),
    }
    tracing::info!(
        "分類標注平台 v{}｜資料庫 {}",
        env!("CARGO_PKG_VERSION"),
        config.database_path.display()
    );
    let bind = config.bind.clone();
    let state = build_state(config).await?;
    let listener = TcpListener::bind(&bind)
        .await
        .with_context(|| format!("無法監聽 {bind}"))?;
    tracing::info!("網頁介面：http://{}", listener.local_addr()?);
    axum::serve(listener, app(state))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    tracing::info!("已停止");
    Ok(())
}

/// `+08:00`、`-0530`（時:分）或 `480`（分鐘）都可以。
fn parse_offset(s: &str) -> Option<i32> {
    let s = s.trim();
    let (sign, rest) = match s.chars().next()? {
        '+' => (1, &s[1..]),
        '-' => (-1, &s[1..]),
        _ => (1, s),
    };
    if !rest.contains(':') && rest.len() != 4 {
        return rest.parse::<i32>().ok().map(|m| sign * m);
    }
    let digits: String = rest.chars().filter(char::is_ascii_digit).collect();
    if digits.len() != 4 || rest.chars().any(|c| !c.is_ascii_digit() && c != ':') {
        return None;
    }
    let h: i32 = digits[..2].parse().ok()?;
    let m: i32 = digits[2..].parse().ok()?;
    Some(sign * (h * 60 + m))
}

async fn import_legacy(config: Config, args: &[String]) -> Result<()> {
    let Some(path) = args.first() else { bail!("{USAGE}") };
    let mut name = None;
    let mut offset = chrono::Local::now().offset().local_minus_utc() / 60;
    let mut rest = args[1..].iter();
    while let Some(flag) = rest.next() {
        match (flag.as_str(), rest.next()) {
            ("--name", Some(v)) => name = Some(v.clone()),
            ("--utc-offset", Some(v)) => offset = parse_offset(v).with_context(|| format!("看不懂時區：{v}"))?,
            _ => bail!("看不懂的選項：{flag}\n\n{USAGE}"),
        }
    }
    let file_name = std::path::Path::new(path)
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();
    let name = name.unwrap_or_else(|| format!("舊版資料（{file_name}）"));
    let state = build_state(config).await?;
    let r = legacy::import_db(&state.db, path.as_ref(), &name, offset).await?;
    let orphans = if r.orphan_labels > 0 {
        format!("，{} 筆找不到對應文字的標注已略過", r.orphan_labels)
    } else {
        String::new()
    };
    println!(
        "已匯入成專案 #{}「{name}」：{} 筆文字（其中 {} 筆隱藏）、{} 筆標注{orphans}",
        r.project_id, r.items, r.hidden_items, r.annotations
    );
    if !r.added_options.is_empty() {
        println!(
            "舊資料裡有舊版介面沒有的選項，已經加進專案：{}",
            r.added_options.join("、")
        );
    }
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

#[cfg(test)]
mod tests {
    use super::parse_offset;

    #[test]
    fn offsets() {
        assert_eq!(parse_offset("+08:00"), Some(480));
        assert_eq!(parse_offset("-0530"), Some(-330));
        assert_eq!(parse_offset("480"), Some(480));
        assert_eq!(parse_offset("abc"), None);
    }
}
