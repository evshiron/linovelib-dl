use clap::Parser;

mod linovelib;

#[derive(Parser, Debug)]
#[clap(about, version, author)]
struct Args {
  novel_id: String,
}

#[tokio::main]
async fn main() {
  let args = Args::parse();

  let downloader = linovelib::Downloader::new();
  downloader.run(&args.novel_id).await;
}
