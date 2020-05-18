use anyhow::Context as _;
use std::{
    borrow::Cow,
    fmt, fs, io,
    net::{TcpListener, TcpStream},
    path::Path,
};

use ring_rt::runtime::{Handle, Runtime};

fn main() -> anyhow::Result<()> {
    let mut rt = Runtime::new().context("failed to start executor")?;
    let handle = rt.handle().clone();
    rt.block_on(main_async(&handle))
}

async fn main_async(handle: &Handle) -> anyhow::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:8000")?;

    loop {
        let (stream, _addr) = handle
            .submit(ring_rt::event::accept(&listener, libc::SOCK_CLOEXEC))
            .await?;
        handle.spawn(handle_connection(handle.clone(), stream));
    }
}

async fn handle_connection(handle: Handle, stream: TcpStream) -> anyhow::Result<()> {
    let raw_request = loop {
        let (mut buf, res) = handle
            .submit(ring_rt::event::read(&stream, vec![0u8; 8196], 0))
            .await;
        let n = res?;
        assert!(n <= buf.len());
        unsafe {
            buf.set_len(n);
        }
        if n == 0 {
            continue;
        }
        break buf;
    };

    let mut headers = [httparse::EMPTY_HEADER; 16];
    let mut request = httparse::Request::new(&mut headers);
    let _amt = match request.parse(&raw_request)? {
        httparse::Status::Complete(amt) => amt,
        httparse::Status::Partial => anyhow::bail!("partial request"),
    };
    let method = request.method.context("missing HTTP method")?;
    let path = request.path.context("missing HTTP path")?;

    let (response, body) = handle_request(method, path).await.unwrap_or_else(|err| {
        make_error_response(
            "500 Internal Server Error",
            &format!("internal server error: {}", err),
        )
    });

    let (_, res) = handle
        .submit(ring_rt::event::write(
            &stream,
            response.to_string().into(),
            0,
        ))
        .await;
    res?;

    let (_, res) = handle.submit(ring_rt::event::write(&stream, body, 0)).await;
    res?;

    Ok(())
}

struct Response {
    status: &'static str,
    headers: Vec<(&'static str, Cow<'static, str>)>,
}

impl fmt::Display for Response {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HTTP/1.1 {}\r\n", self.status)?;
        for (name, value) in &self.headers {
            write!(f, "{}: {}\r\n", name, value)?;
        }
        write!(f, "\r\n")?;
        Ok(())
    }
}

async fn handle_request(method: &str, path: &str) -> anyhow::Result<(Response, Vec<u8>)> {
    match method {
        "GET" => {
            // FIXME: asyncify

            let is_dir = path.ends_with('/');
            let mut file_path = Path::new("public").join(&path[1..]);
            if is_dir {
                file_path.push("index.html");
            }

            let file = match fs::OpenOptions::new().read(true).open(&file_path) {
                Ok(f) => f,
                Err(err) if err.kind() == io::ErrorKind::NotFound => {
                    return Ok(make_error_response(
                        "404 Not Found",
                        &format!("Not Found: {}", path),
                    ));
                }
                Err(err) => anyhow::bail!(err),
            };

            let metadata = file.metadata()?;
            let content_type = match file_path.extension().and_then(|ext| ext.to_str()) {
                Some("jpg") | Some("jpeg") => "image/jpg",
                Some("png") => "image/png",
                Some("gif") => "image/gif",
                Some("html") | Some("htm") => "text/html",
                Some("js") => "application/javascript",
                Some("css") => "text/css",
                Some("txt") => "text/plain",
                Some("json") => "application/json",
                _ => "text/plain",
            };

            let response = Response {
                status: "200 OK",
                headers: vec![
                    ("content-type", content_type.into()),
                    ("content-length", metadata.len().to_string().into()),
                ],
            };

            let mut content = Vec::with_capacity(metadata.len() as usize);
            use std::io::Read as _;
            io::BufReader::new(file).read_to_end(&mut content)?;

            Ok((response, content))
        }
        _ => Ok(make_error_response(
            "400 Bad Request",
            "unimplemented HTTP method",
        )),
    }
}

fn make_error_response(status: &'static str, msg: &str) -> (Response, Vec<u8>) {
    let body = format!(
        "\
            <html>\
            <head>\
            <title>{status}</title>\
            </head>\
            <body>\
            <h1>{status}</h1>\
            <p>{msg}</p>\
            </body>\
            </html>\
        ",
        status = status,
        msg = msg,
    );
    let body_len = body.len().to_string();
    (
        Response {
            status,
            headers: vec![
                ("content-type", "text/html".into()),
                ("content-length", body_len.into()),
            ],
        },
        body.into(),
    )
}
