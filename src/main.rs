use pca9956b_api::{ApiNoContext, ContextWrapperExt};
use tokio_core::{reactor, reactor::Core};
use clap::{App, Arg};
use swagger::{make_context,make_context_ty};
use swagger::{ContextBuilder, EmptyContext, XSpanIdString, Push, AuthData};
use uuid;
use hyper;

struct Config {
    https: bool,
    host: String,
    port: String,
    bus: i32,
    addr: i32,
}

type ClientContext = make_context_ty!(ContextBuilder, EmptyContext, Option<AuthData>, XSpanIdString);
type Client<'a> = swagger::context::ContextWrapper<'a, pca9956b_api::client::Client<hyper::client::FutureResponse>, ClientContext>;

fn main() {
    let conf = get_args();
    dump_args(&conf);
    let mut core = reactor::Core::new().unwrap();
    let client = create_client(&conf, &core);
    let client = client.with_context(make_context!(ContextBuilder, EmptyContext, None as Option<AuthData>, XSpanIdString(self::uuid::Uuid::new_v4().to_string())));

    run(&conf, &mut core, &client)
}

fn get_args() -> Config {
    let matches = App::new("pca9956b-cli")
        .arg(Arg::with_name("https")
            .long("https")
            .help("Whether to use HTTPS or not"))
        .arg(Arg::with_name("host")
            .long("host")
            .takes_value(true)
            .default_value("localhost")
            .help("Hostname to contact"))
        .arg(Arg::with_name("port")
            .long("port")
            .takes_value(true)
            .default_value("80")
            .help("Port to contact"))
        .arg(Arg::with_name("bus")
            .long("bus")
            .takes_value(true)
            .default_value("0")
            .help("I2C Bus ID"))
        .arg(Arg::with_name("addr")
            .long("addr")
            .takes_value(true)
            .default_value("0")
            .help("PCA9956B I2C address"))
        .get_matches();

    Config {
        https: matches.is_present("https"),
        host: matches.value_of("host").unwrap().to_string(),
        port: matches.value_of("port").unwrap().to_string(),
        bus: matches.value_of("bus").unwrap().parse::<i32>().unwrap(),
        addr: matches.value_of("addr").unwrap().parse::<i32>().unwrap(),
    }
}

fn dump_args(conf: &Config) {
  println!("Args");
  println!("  https: {}", conf.https);
  println!("  host:  {}", conf.host);
  println!("  port:  {}", conf.port);
  println!("  bus:   {}", conf.bus);
  println!("  addr:  {}", conf.addr);
}

fn create_client<'a>(conf: &Config, core: &Core) -> pca9956b_api::client::Client<hyper::client::FutureResponse> {
    let base_url = format!("{}://{}:{}",
                           if conf.https { "https" } else { "http" },
                           conf.host,
                           conf.port);
    if conf.https {
        pca9956b_api::Client::try_new_https(core.handle(), &base_url, "examples/ca.pem")
            .expect("Failed to create HTTPS client")
    } else {
        pca9956b_api::Client::try_new_http(core.handle(), &base_url)
            .expect("Failed to create HTTP client")
    }
}

fn run(conf: &Config, mut core: &mut Core, client: &Client) {
    // Sequence is:
    // - Get current device state
    // - Output that state
    // - Wait for an action from the user
    // - Apply that action, outputting result
    // - Start again from the top
    get_api(&mut core, &client);
    get_info(&conf, &mut core, &client);
}

fn get_api(core: &mut Core, client: &Client) {
    let result = core.run(client.get_api());
    println!("get_api ...");
    println!("{:?}", result);
}

fn get_info(conf: &Config, core: &mut Core, client: &Client) {
    let result = core.run(client.get_led_info_all(conf.bus, conf.addr));
    println!("get_info ...");
    println!("{:?}", result);
}