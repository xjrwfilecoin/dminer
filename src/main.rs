use warp::Filter;
use clap::Arg;
use warp::hyper::body::Bytes;

use filecoin_proofs_api::seal::{seal_pre_commit_phase1,seal_pre_commit_phase2,seal_commit_phase1,seal_commit_phase2};
use filecoin_proofs_api::post::{generate_window_post,generate_winning_post};
use filecoin_proofs_api::{PieceInfo, UnpaddedBytesAmount, RegisteredSealProof, ProverId, SectorId, Ticket, Commitment, SectorSize};
use bincode::{deserialize,serialize};
use tokio::time::Duration;
use std::convert::Infallible;

// mod post;
// mod polling;
// mod system;
// mod seal;


#[tokio::main]
async fn main() {
    let m =
    clap::App::new("Filecoin-miner")
        .author("Paul Wang<wangpy@jxrw.com>")
        .version("1.0.0")
        .about("filecoin-miner")
        .arg(
            Arg::with_name("port")
                .short("p")
                .long("port")
                .help("server port")
                .default_value("9330")
        )
        .get_matches();

    env_logger::init();

    let root = warp::path::end().map(|| "Filecoin miner");

    let pre_seal_phase1 = warp::path("phase1")
        .and(warp::body::content_length_limit(1024 * 32))
        .and(warp::body::bytes())
        .map(|body:Bytes| {
            let data = body.to_vec();
            let req = serde_json::Value::from(String::from_utf8(data).unwrap());

            let registered_proof = base64::decode(req["proof"].as_str().unwrap()).unwrap();
            let registered_proof:RegisteredSealProof = deserialize(&registered_proof[..]).unwrap();
            let cache_path = req["cache-path"].as_str().unwrap();
            let in_path = req["in-path"].as_str().unwrap();
            let out_path = req["out-path"].as_str().unwrap();
            let prover_id = base64::decode(req["prover-id"].as_str().unwrap()).unwrap();
            let prover_id:ProverId = deserialize(&prover_id[..]).unwrap();

            let sector_id = base64::decode(req["sector-id"].as_str().unwrap()).unwrap();
            let sector_id:SectorId = deserialize(&sector_id[..]).unwrap();

            let ticket = base64::decode(req["ticket"].as_str().unwrap()).unwrap();
            let ticket:Ticket = deserialize(&ticket[..]).unwrap();
            let infos = req["piece-infos"].as_array().unwrap();

            let mut pieces = vec![];
            for item in infos {
                let commitment = item["commitment"].as_str().unwrap();
                let mut comm = base64::decode(commitment).unwrap();
                comm.truncate(32);
                let mut commitment:Commitment = [0;32];
                commitment.copy_from_slice(&comm[..]);

                let size = item["size"].as_u64().unwrap();
                let piece:PieceInfo = PieceInfo {
                    commitment,
                    size: UnpaddedBytesAmount::from(SectorSize(size))
                };
                pieces.push(piece);
            }

            let out =
            seal_pre_commit_phase1(
                registered_proof,
                cache_path,
                in_path,
                out_path,
                prover_id,
                sector_id,
                ticket,
                &pieces[..]
            ).unwrap();
            let out = serialize(&out).unwrap();
            let rsp = serde_json::json!({
                "len":out.len(),
                "data":base64::encode(out).as_str()
            });
            log::warn!(">{}",serde_json::to_string_pretty(&rsp).unwrap());
            warp::reply::json(&rsp)
        });

    let pre_seal_phase2 = warp::path("phase2").map(|| {
        log::warn!("sleepy..");
         tokio::spawn(async {
             sleepy().await.unwrap();
             log::warn!("sleepy!!");
         });
        log::warn!("sleepy..");
        "pre commit phase2"
    });

    let pre_seal = warp::path("pre").and(pre_seal_phase1.or(pre_seal_phase2));


    let winning_post = warp::path("winning").map(|| "winning post");
    let window_post = warp::path("window").map(|| "window post");

    let post = warp::path::end().map(|| "post");

    let post = warp::path("post").and(winning_post.or(window_post).or(post));

    let api = warp::path("api").and(pre_seal.or(post).or(warp::path::end().map(||"api")));

    let api2 = filters::api();

    let routes = api2.with(warp::log("filecoin"));

    // let routes = warp::any().and(
    //     api
    //         // .or(sealing.and(pre_seal_phase1.or(pre_seal_phase2)))
    //         .or(root),
    // );
    warp::serve(routes)
        .run(([127, 0, 0, 1], 3030))
        .await;
}


mod filters {
    use warp::{Filter, Reply, Rejection};

    use super::handlers::pre_commit_phase1;

    pub fn api() -> impl Filter<Extract=impl Reply,Error = Rejection> + Clone {
        warp::path("api").and(warp::any()).and(pre().or(warp::path::end().map(|| "home")))
    }

    pub fn pre() -> impl Filter<Extract=impl Reply,Error = Rejection> + Clone {
       warp::path("pre").and(warp::any()).and(warp::path::end().map(|| "pre home").or(pre_commit_phase_1()).or(pre_commit_phase_2()))
    }

    pub fn pre_commit_phase_1() -> impl Filter<Extract = impl Reply,Error = Rejection> + Clone {
        warp::path("phase1")
            .and(warp::post())
            .and(warp::body::content_length_limit(64 * 1024))
            .and(warp::body::bytes())
            .and_then(pre_commit_phase1)
    }

    pub fn pre_commit_phase_2() -> impl Filter<Extract = impl Reply,Error = Rejection> + Clone {
        warp::path("phase2")
            .and(warp::post())
            .and(warp::body::content_length_limit(64 * 1024))
            .and(warp::body::bytes())
            .and_then(pre_commit_phase1)
    }
}

mod handlers {
    use warp::Reply;
    use std::convert::Infallible;

    pub async fn pre_commit_phase1(_body:bytes::Bytes) -> Result<impl Reply,Infallible> {
        let rsp = serde_json::json!({
           "status":200,
           "data":{

           }
        });
        Ok(warp::reply::json(&rsp))
    }
}

mod models {

}

#[cfg(test)]
mod tests {

}


async fn sleepy() ->Result<impl warp::Reply,Infallible> {
    tokio::time::sleep(Duration::from_secs(10)).await;
    Ok(format!("I waited 10s!"))
}