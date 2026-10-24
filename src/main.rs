mod app;
mod md;
mod model;
mod ops;
mod render;
mod util;
mod validate;
mod xmind;

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use serde_json::json;

use crate::util::AppError;

#[derive(Parser)]
#[command(
    name = "xmind-cli-lite",
    bin_name = "xmind-cli-lite",
    version,
    about = "离线创建、读取、编辑、校验 Xmind 2020+ 文件"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Markdown -> .xmind
    Create(CreateArgs),
    /// 读取为 JSON 或 outline
    Read(ReadArgs),
    /// 输出结构摘要，永远 JSON
    Describe(DescribeArgs),
    /// 列出 sheet
    Sheets(SheetsArgs),
    /// 校验文件结构与资源
    Validate(ValidateArgs),
    /// 执行一批编辑操作
    Batch(BatchArgs),
    /// 设置/清除主题链接
    Link(LinkArgs),
    /// 设置/清除主题图片
    Image(ImageArgs),
}

#[derive(Clone, Copy, ValueEnum)]
pub enum Format {
    Json,
    Outline,
}

#[derive(clap::Args)]
pub struct CreateArgs {
    /// Markdown 文件
    #[arg(value_name = "DRAFT")]
    pub draft: Option<PathBuf>,
    /// 从 stdin 读取 Markdown
    #[arg(long)]
    pub stdin: bool,
    /// 输出 .xmind
    #[arg(short = 'o', long = "out", value_name = "OUT")]
    pub out: PathBuf,
    /// 骨架别名或官方名
    #[arg(long, default_value = "mindmap")]
    pub skeleton: String,
    /// sheet 标题
    #[arg(long)]
    pub sheet: Option<String>,
    /// 覆盖 H1；Markdown 无 H1 时必填
    #[arg(long)]
    pub title: Option<String>,
    /// 只规划不落盘
    #[arg(long)]
    pub dry_run: bool,
    /// 覆盖已存在的输出
    #[arg(long)]
    pub force: bool,
    /// 跳过自动校验
    #[arg(long)]
    pub no_validate: bool,
}

#[derive(clap::Args)]
pub struct ReadArgs {
    pub file: PathBuf,
    /// sheet id 或标题；默认全部
    #[arg(long)]
    pub sheet: Option<String>,
    /// 输出格式
    #[arg(long, value_enum, default_value_t = Format::Json)]
    pub format: Format,
    /// outline 最大层级（根为 0）
    #[arg(long)]
    pub depth: Option<usize>,
}

#[derive(clap::Args)]
pub struct DescribeArgs {
    pub file: PathBuf,
    #[arg(long)]
    pub sheet: Option<String>,
}

#[derive(clap::Args)]
pub struct SheetsArgs {
    pub file: PathBuf,
}

#[derive(clap::Args)]
pub struct ValidateArgs {
    pub file: PathBuf,
    /// 警告升级为错误
    #[arg(long)]
    pub strict: bool,
    /// 只返回退出码
    #[arg(long)]
    pub quiet: bool,
    /// JSON 输出
    #[arg(long)]
    pub json: bool,
}

#[derive(clap::Args)]
pub struct BatchArgs {
    pub file: PathBuf,
    /// ops JSON 文件；省略或 `-` 时读 stdin
    #[arg(long)]
    pub input: Option<PathBuf>,
    #[arg(long)]
    pub sheet: Option<String>,
    /// 写到副本而不是原地修改
    #[arg(long)]
    pub out: Option<PathBuf>,
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long)]
    pub backup: bool,
    #[arg(long)]
    pub force: bool,
    #[arg(long)]
    pub no_validate: bool,
}

#[derive(clap::Args)]
pub struct LinkArgs {
    #[arg(value_name = "FILE")]
    pub xmind: PathBuf,
    /// 精确且唯一的主题标题
    #[arg(long)]
    pub topic: String,
    #[arg(long)]
    pub url: Option<String>,
    /// 本地文件，按当前目录解析
    #[arg(long = "file", value_name = "PATH")]
    pub target_file: Option<PathBuf>,
    /// 清除链接
    #[arg(long)]
    pub remove: bool,
    #[arg(long)]
    pub sheet: Option<String>,
}

#[derive(clap::Args)]
pub struct ImageArgs {
    #[arg(value_name = "FILE")]
    pub xmind: PathBuf,
    /// 精确且唯一的主题标题
    #[arg(long)]
    pub topic: String,
    /// 本地图片
    #[arg(long)]
    pub input: Option<PathBuf>,
    /// 清除图片
    #[arg(long)]
    pub remove: bool,
    /// 覆盖默认宽度
    #[arg(long)]
    pub width: Option<u32>,
    #[arg(long)]
    pub sheet: Option<String>,
}

fn main() {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error)
            if matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
            ) =>
        {
            eprint!("{error}");
            std::process::exit(2);
        }
        Err(error) if error.use_stderr() => {
            print_error(&AppError::new("usage", 2, error.to_string().trim_end()));
            std::process::exit(2);
        }
        Err(error) => {
            print!("{error}");
            std::process::exit(0);
        }
    };
    let code = match dispatch(cli.command) {
        Ok(code) => code,
        Err(error) => {
            print_error(&error);
            error.exit
        }
    };
    std::process::exit(code);
}

fn dispatch(command: Command) -> util::Result<i32> {
    match command {
        Command::Create(args) => app::create(args),
        Command::Read(args) => app::read(args),
        Command::Describe(args) => app::describe(args),
        Command::Sheets(args) => app::sheets(args),
        Command::Validate(args) => app::validate(args),
        Command::Batch(args) => app::batch(args),
        Command::Link(args) => app::link(args),
        Command::Image(args) => app::image(args),
    }
}

fn print_error(error: &AppError) {
    let value = json!({
        "ok": false,
        "error": { "code": error.code, "message": error.message },
    });
    println!(
        "{}",
        serde_json::to_string(&value).unwrap_or_else(|_| "{\"ok\":false}".to_string())
    );
    eprintln!("{}", error.message);
}
