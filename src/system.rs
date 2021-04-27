use crate::config::Config;
use crate::polling::*;
use actix_multipart::Multipart;
use actix_web::web::{self, Data, Json};
use actix_web::{Error, HttpResponse};
use futures::stream::{StreamExt, TryStreamExt};
use lazy_static::lazy_static;
use libc::pthread_cancel;
use log::*;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Write;
use std::os::unix::thread::JoinHandleExt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

lazy_static! {
    static ref WORKER_TOKEN: AtomicU64 = AtomicU64::new(0);
    static ref WORKER_INIT: AtomicBool = AtomicBool::new(false);
}

#[derive(Serialize, Debug)]
struct ServerLoad {
    limit: u64,
    current: u64,
}

pub struct WorkerProp {
    name: String,
    handle: JoinHandle<()>,
    receiver: Receiver<Value>,
}

impl WorkerProp {
    pub fn new(name: String, handle: JoinHandle<()>, receiver: Receiver<Value>) -> Self {
        Self { name, handle, receiver }
    }
}

pub struct ServState {
    workers: HashMap<u64, WorkerProp>,
    config: Config,
}

impl ServState {
    pub fn new(config: Config) -> Self {
        // NOTE: ensure ServState is init only once
        assert_eq!(WORKER_INIT.swap(true, Ordering::SeqCst), false);

        Self {
            workers: HashMap::new(),
            config,
        }
    }

    pub fn verify_token<S: AsRef<str>>(&self, token: S) -> bool {
        self.config.allow_tokens.contains(&token.as_ref().to_owned())
    }

    pub fn job_num<S: AsRef<str>>(&self, name: S) -> u64 {
        self.workers
            .iter()
            .filter(|(_, prop)| prop.name == name.as_ref())
            .count() as u64
    }

    pub fn job_limit<S: AsRef<str>>(&self, name: S) -> u64 {
        *self.config.job_limits.get(name.as_ref()).unwrap_or(&u64::max_value())
    }

    pub fn job_available<S: AsRef<str>>(&self, name: S) -> bool {
        let num = self.job_num(name.as_ref());
        let limit = self.job_limit(name.as_ref());

        num < limit
    }

    pub fn enqueue(&mut self, prop: WorkerProp) -> PollingState {
        let token = WORKER_TOKEN.fetch_add(1, Ordering::SeqCst);
        self.workers.insert(token, prop);

        PollingState::Started(token)
    }

    pub fn get(&mut self, token: u64) -> PollingState {
        let state = self
            .workers
            .get(&token)
            .map(|x| match x.receiver.try_recv() {
                Ok(r) => PollingState::Done(r),
                Err(TryRecvError::Empty) => PollingState::Pending,
                Err(TryRecvError::Disconnected) => PollingState::Error(PollingError::Disconnected),
            })
            .unwrap_or(PollingState::Error(PollingError::NotExist));

        match &state {
            PollingState::Done(_) => {
                debug!("Job {} removed dut to finish", token);
                self.workers.remove(&token);
            }
            _ => {}
        };

        state
    }

    pub fn remove(&mut self, token: u64) -> PollingState {
        if let Some(prop) = self.workers.remove(&token) {
            debug!("Job {} force removed", token);
            let pthread_t = prop.handle.into_pthread_t();

            unsafe {
                pthread_cancel(pthread_t);
            }

            return PollingState::Removed;
        }

        PollingState::Error(PollingError::NotExist)
    }
}

pub async fn test() -> HttpResponse {
    trace!("test");

    HttpResponse::Ok().body("Worked!")
}

pub async fn test_polling(state: Data<Arc<Mutex<ServState>>>) -> HttpResponse {
    trace!("test polling");

    let (tx, rx) = channel();
    let handle: JoinHandle<()> = thread::spawn(move || {
        thread::sleep(Duration::from_secs(30));
        let r = "Ok!!!";

        tx.send(json!(r)).unwrap();
    });

    let prop = WorkerProp::new("Test".to_string(), handle, rx);
    let response = state.lock().unwrap().enqueue(prop);
    HttpResponse::Ok().json(response)
}

pub async fn query_load(state: Data<Arc<Mutex<ServState>>>, phase: Json<String>) -> HttpResponse {
    trace!("query_load: {:?}", phase);

    let data = {
        let state = state.lock().unwrap();

        ServerLoad {
            limit: state.job_limit(&phase.0),
            current: state.job_num(&phase.0),
        }
    };

    debug!("current load for {}, {:?}", &phase.0, data);

    HttpResponse::Ok().json(data)
}

pub async fn query_state(state: Data<Arc<Mutex<ServState>>>, token: Json<u64>) -> HttpResponse {
    trace!("query_state: {:?}", token);

    let response = state.lock().unwrap().get(*token);

    HttpResponse::Ok().json(response)
}

pub async fn remove_job(state: Data<Arc<Mutex<ServState>>>, token: Json<u64>) -> HttpResponse {
    trace!("remove_job: {:?}", token);

    let response = state.lock().unwrap().remove(*token);

    HttpResponse::Ok().json(response)
}

pub async fn upload_file(mut payload: Multipart) -> Result<HttpResponse, Error> {
    trace!("upload_file");

    let mut ret_path: Option<String> = None;

    // iterate over multipart stream
    while let Ok(Some(mut field)) = payload.try_next().await {
        let content_type = field.content_disposition().unwrap();
        let filename = content_type.get_filename().unwrap();
        let filepath = format!("/tmp/upload/{}", filename);
        trace!("got file: {}", filepath);
        ret_path = Some(filepath.clone());

        // File::create is blocking operation, use threadpool
        let mut f = web::block(|| std::fs::File::create(filepath)).await.unwrap();

        // Field in turn is stream of *Bytes* object
        while let Some(chunk) = field.next().await {
            let data = chunk.unwrap();
            // filesystem operations are blocking, we have to use threadpool
            f = web::block(move || f.write_all(&data).map(|_| f)).await?;
        }
    }

    // TODO: file name
    Ok(HttpResponse::Ok().json(ret_path))
}

pub async fn upload_test() -> HttpResponse {
    let html = r#"<html>
        <head><title>Upload Test</title></head>
        <body>
            <form action="/sys/upload_file" target="/sys/upload_file" method="post" enctype="multipart/form-data">
                <input type="file" multiple name="file"/>
                <input type="submit" value="Submit">
            </form>
        </body>
	    </html>"#;

    HttpResponse::Ok().body(html)
}
