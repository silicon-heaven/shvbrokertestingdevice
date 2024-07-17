use std::sync::atomic::{AtomicI32, Ordering};
use async_std::sync::RwLock;

use clap::Parser;
use log::*;
use shvrpc::{client::ClientConfig, util::parse_log_verbosity};
use shvrpc::{RpcMessage, RpcMessageMetaTags};
use shvclient::appnodes::{DotAppNode, DotDeviceNode};
use shvclient::clientnode::{ClientNode, SIG_CHNG};
use shvclient::{AppState};
use simple_logger::SimpleLogger;

#[derive(Parser, Debug)]
//#[structopt(name = "device", version = env!("CARGO_PKG_VERSION"), author = env!("CARGO_PKG_AUTHORS"), about = "SHV call")]
struct Opts {
    /// Config file path
    #[arg(long)]
    config: Option<String>,
    /// Create default config file if one specified by --config is not found
    #[arg(short, long)]
    create_default_config: bool,
    ///Url to connect to, example tcp://localhost:3755?user=admin&password=dj4j5HHb, localsocket:path/to/socket
    #[arg(short = 's', long = "url")]
    url: Option<String>,
    #[arg(short = 'i', long)]
    device_id: Option<String>,
    /// Mount point on broker connected to, note that broker might not accept any path.
    #[arg(short, long)]
    mount: Option<String>,
    /// Device tries to reconnect to broker after this interval, if connection to broker is lost.
    /// Example values: 1s, 1h, etc.
    #[arg(short, long)]
    reconnect_interval: Option<String>,
    /// Client should ping broker with this interval. Broker will disconnect device, if ping is not received twice.
    /// Example values: 1s, 1h, etc.
    #[arg(long, default_value = "1m")]
    heartbeat_interval: String,
    /// Verbose mode (module, .)
    #[arg(short, long)]
    verbose: Option<String>,
}

fn init_logger(cli_opts: &Opts) {
    let mut logger = SimpleLogger::new();
    logger = logger.with_level(LevelFilter::Info);
    if let Some(module_names) = &cli_opts.verbose {
        for (module, level) in parse_log_verbosity(module_names, module_path!()) {
            logger = logger.with_module_level(module, level);
        }
    }
    logger.init().unwrap();
}

fn load_client_config(cli_opts: Opts) -> shvrpc::Result<ClientConfig> {
    let mut config = if let Some(config_file) = &cli_opts.config {
        ClientConfig::from_file_or_default(config_file, cli_opts.create_default_config)?
    } else {
        Default::default()
    };
    config.url = cli_opts.url.unwrap_or(config.url);
    config.device_id = cli_opts.device_id.or(config.device_id);
    config.mount = cli_opts.mount.or(config.mount);
    config.reconnect_interval = cli_opts.reconnect_interval.or(config.reconnect_interval);
    config.heartbeat_interval.clone_from(&cli_opts.heartbeat_interval);
    Ok(config)
}

struct State {
    number: AtomicI32,
    text: RwLock<String>,
}
const NUMBER_MOUNT: &str = "state/number";
const TEXT_MOUNT: &str = "state/text";
#[async_std::main]
pub(crate) async fn main() -> shvrpc::Result<()> {
    let cli_opts = Opts::parse();
    init_logger(&cli_opts);

    log::info!("=====================================================");
    log::info!("{} starting", std::module_path!());
    log::info!("=====================================================");

    let client_config = load_client_config(cli_opts).expect("Invalid config");

    let state = AppState::new(State{ number: 0.into(), text: "".to_string().into() });

    let number_node: ClientNode<State> = shvclient::fixed_node!{
        number_node_handler(request, client_cmd_tx, app_state: State) {
            "get" [IsGetter, Read] => {
                    Some(Ok(app_state.number.load(Ordering::SeqCst).into()))
            }
            "set" [IsSetter, Write] (param: i32) => {
                if app_state.number.load(Ordering::SeqCst) != param {
                    app_state.number.store(param, Ordering::SeqCst);
                    let sigchng = RpcMessage::new_signal(NUMBER_MOUNT, SIG_CHNG, Some(param.into()));
                    let _ = client_cmd_tx.send_message(sigchng);
                }
                Some(Ok(true.into()))
            }
       }
    };
    let text_node = shvclient::fixed_node!{
        text_node_handler(request, client_cmd_tx, app_state: State) {
            "get" [IsGetter, Read] => {
                let s = &*app_state.text.read().await;
                Some(Ok(s.into()))
            }
            "set" [IsSetter, Write] (param: String) => {
                if &*app_state.text.read().await != &param {
                    let mut writer = app_state.text.write().await;
                    *writer = param.clone();
                    let sigchng = RpcMessage::new_signal(TEXT_MOUNT, SIG_CHNG, Some(param.into()));
                    let _ = client_cmd_tx.send_message(sigchng);
                }
                Some(Ok(true.into()))
            }
       }
    };

    //let init_task = move |client_cmd_tx, client_evt_rx| {
    //};

    shvclient::Client::new_device(DotAppNode::new("simple_device_async_std"), DotDeviceNode::new("shvbroker_testing_device", "0.1", Some("00000".into())))
        .mount(NUMBER_MOUNT, number_node)
        .mount(TEXT_MOUNT, text_node)
        .with_app_state(state)
        //.run_with_init(&client_config, init_task)
        .run(&client_config)
        .await
}