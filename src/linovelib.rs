use std::sync::Arc;

const USER_AGENT: &'static str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/96.0.4664.93 Safari/537.36";
const DATA_DIR: &'static str = "./data";

#[derive(Debug)]
enum DownloadCommand {
  Catalog {
    novel_id: String,
  },
  Chapter {
    novel_id: String,
    filename: String,
  },
  Image {
    novel_id: String,
    filename: String,
    image_url: String,
  },
  Completed,
}

pub struct Downloader {
  http_client: reqwest::Client,

  command_ch: tokio::sync::OnceCell<tokio::sync::mpsc::UnboundedSender<DownloadCommand>>,
}

impl Downloader {
  pub fn new() -> Arc<Self> {
    Arc::new(Self {
      http_client: reqwest::Client::new(),

      command_ch: tokio::sync::OnceCell::new(),
    })
  }

  async fn fetch_catalog(&self, novel_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let url = format!("https://w.linovelib.com/novel/{}/catalog", novel_id);
    let data = self
      .http_client
      .get(url)
      .header("user-agent", USER_AGENT)
      .send()
      .await?
      .text()
      .await?;

    let novel_dir = std::path::PathBuf::from(DATA_DIR).join(novel_id);

    tokio::fs::create_dir_all(&novel_dir).await?;

    let catelog = novel_dir.join("catalog");

    tokio::fs::write(catelog, &data).await?;

    let doc = scraper::Html::parse_document(&data);

    let first_chapter = doc
      .select(&scraper::Selector::parse("a.chapter-li-a").unwrap())
      .next()
      .unwrap();
    let first_chapter_url = first_chapter.value().attr("href").unwrap();

    let first_chapter_filename = first_chapter_url.split("/").last().unwrap();

    self
      .command_ch
      .get()
      .unwrap()
      .send(DownloadCommand::Chapter {
        novel_id: novel_id.to_string(),
        filename: first_chapter_filename.to_string(),
      })
      .unwrap();

    Ok(())
  }

  async fn fetch_chapter(
    &self,
    novel_id: &str,
    filename: &str,
  ) -> Result<(), Box<dyn std::error::Error>> {
    let url = format!("https://w.linovelib.com/novel/{}/{}", novel_id, filename);

    let data = self
      .http_client
      .get(url)
      .header("user-agent", USER_AGENT)
      .send()
      .await?
      .text()
      .await?;

    let novel_dir = std::path::PathBuf::from(DATA_DIR).join(novel_id);

    let path = novel_dir.join(filename);

    tokio::fs::write(path, &data).await?;

    let doc = scraper::Html::parse_document(&data);

    for img in doc.select(&scraper::Selector::parse("div.divimage img").unwrap()) {
      let image_url = img.value().attr("src").unwrap_or("");
      if !image_url.is_empty() {
        self
          .command_ch
          .get()
          .unwrap()
          .send(DownloadCommand::Image {
            novel_id: novel_id.to_string(),
            filename: filename.to_string(),
            image_url: image_url.to_string(),
          })
          .unwrap();
      }
    }

    let chapter_name = doc
      .select(&scraper::Selector::parse("h3").unwrap())
      .next()
      .unwrap()
      .text()
      .next()
      .unwrap();

    let sub_chapter_name = doc
      .select(&scraper::Selector::parse("h1").unwrap())
      .next()
      .unwrap()
      .text()
      .next()
      .unwrap();

    println!(
      "chapter {}, {}, {}, {} saved",
      novel_id, filename, chapter_name, sub_chapter_name
    );

    let next_url_regex = regex::Regex::new(r"url_next:'([^']+)'").unwrap();
    for next_url_capture in next_url_regex.captures_iter(&data) {
      let next_chapter_url = next_url_capture.get(1).unwrap().as_str();

      let next_chapter_filename = next_chapter_url.split("/").last().unwrap();

      if next_chapter_filename != "catalog" {
        self
          .command_ch
          .get()
          .unwrap()
          .send(DownloadCommand::Chapter {
            novel_id: novel_id.to_string(),
            filename: next_chapter_filename.to_string(),
          })
          .unwrap();
      } else {
        self
          .command_ch
          .get()
          .unwrap()
          .send(DownloadCommand::Completed)
          .unwrap();
      }
    }

    Ok(())
  }

  async fn fetch_image(
    &self,
    novel_id: &str,
    source: &str,
    image_url: &str,
  ) -> Result<(), Box<dyn std::error::Error>> {
    let data = self
      .http_client
      .get(image_url)
      .header("user-agent", USER_AGENT)
      .send()
      .await?
      .bytes()
      .await?;

    let novel_dir = std::path::PathBuf::from(DATA_DIR).join(novel_id);

    let filename = image_url.split("/").last().unwrap();

    let path = novel_dir.join(format!("{}_{}", source, filename));

    tokio::fs::write(path, &data).await?;

    println!("image {}, {}, {} saved", novel_id, source, image_url);

    Ok(())
  }

  async fn command_handler(
    self: Arc<Self>,
    mut command_rx: tokio::sync::mpsc::UnboundedReceiver<DownloadCommand>,
  ) {
    loop {
      match command_rx.recv().await {
        Some(command) => match command {
          DownloadCommand::Catalog { novel_id } => {
            self.fetch_catalog(&novel_id).await.unwrap();
          },
          DownloadCommand::Chapter { novel_id, filename } => {
            self.fetch_chapter(&novel_id, &filename).await.unwrap();
          },
          DownloadCommand::Image {
            novel_id,
            filename,
            image_url,
          } => {
            self
              .fetch_image(&novel_id, &filename, &image_url)
              .await
              .unwrap();
          },
          DownloadCommand::Completed => {
            break;
          },
        },
        None => {
          break;
        },
      }
    }
  }

  pub async fn run(self: Arc<Self>, novel_id: &str) {
    let (command_tx, command_rx) = tokio::sync::mpsc::unbounded_channel::<DownloadCommand>();
    self.command_ch.set(command_tx).unwrap();

    let command_handler = tokio::spawn(self.clone().command_handler(command_rx));

    self
      .command_ch
      .get()
      .unwrap()
      .send(DownloadCommand::Catalog {
        novel_id: novel_id.to_string(),
      })
      .unwrap();

    let _ = tokio::join!(command_handler);
  }
}
