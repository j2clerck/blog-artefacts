use lambda_runtime::{run, service_fn, Error, LambdaEvent};
use tracing::{error};
use serde::Deserialize;
use std::io::Cursor;


#[derive(Deserialize, Debug)]
pub struct EventBridgeEvent {
    detail: EventBridgeDetail,
}

#[derive(Deserialize, Debug)]
struct EventBridgeDetail {
    bucket: EventBridgeBucket,
    object: EventBridgeObject,
}

#[derive(Deserialize, Debug)]
struct EventBridgeBucket {
    name: String,
}
#[derive(Deserialize, Debug)]
struct EventBridgeObject {
    key: String,
    size: u32
}

async fn function_handler(event: LambdaEvent<EventBridgeEvent>) -> Result<(), Error> {
    let (event, _context) = event.into_parts();
    
    let config = aws_config::load_from_env().await;
    let client = aws_sdk_s3::Client::new(&config);
    
    // We retrieve the bucket name and key from the event
    let bucket_name = event.detail.bucket.name;
    let key = event.detail.object.key;
    
    let result = client
        .get_object()
        .bucket(&bucket_name)
        .key(&key)
        .send()
        .await;

    let body = match result {
        Ok(output) => output.body.collect().await.ok().unwrap(),
        Err(e) => {
            error!("Unable to retrieve the object {}", &key);
            return Err(Error::from(e.into_service_error()))
        }
    };

    let image = match image::load_from_memory(&body.into_bytes()){
        Ok(img) => img,
        Err(e) => {
            error!("Unable to process the image {}", &key);
            return Err(Error::from(e.to_string()));
        }
    };
    let thumbnail = image.thumbnail(1280,720);

    // we create the buffer and write the thumbnail into it as a Jpeg image.
    let mut buffer: Vec<u8> = Vec::new();
    thumbnail.write_to(
        &mut Cursor::new(&mut buffer),
        image::ImageOutputFormat::Jpeg(50),
    )?;

    // we generate a key for the thumbnail by prefixing thumbnail and changing the extension to jpeg.
    let split_key: Vec<&str> = key.split('.').collect();
    let thumbnail_key = format!("thumbnail/{}.jpeg", split_key[0]);
    

    // upload the thumbnail to S3
    let resp = client
        .put_object()
        .body(aws_sdk_s3::types::ByteStream::from(buffer))
        .bucket(&bucket_name)
        .key(&thumbnail_key)
        .send()
        .await;
    if resp.is_err() {
        error!("Unable to upload the thumbnail. Error {:?}", resp);
        return Err(Error::from(resp.err().unwrap()));
    };

    let source = format!("{}/{}", &bucket_name, &key);
    let encoded_source = urlencoding::encode(source.as_str());
    let resp = client
        .copy_object()
        .copy_source(encoded_source)
        .key(&key)
        .bucket(&bucket_name)
        .storage_class(aws_sdk_s3::model::StorageClass::DeepArchive)
        .send()
        .await;
    
    if resp.is_err() {
        error!("Unable to upload the thumbnail. Error {:?}", resp);
        return Err(Error::from(resp.err().unwrap()));
    };

    Ok(())

}



#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        // disable printing the name of the module in every log line.
        .with_target(false)
        // disabling time is handy because CloudWatch will add the ingestion time.
        .without_time()
        .init();

    run(service_fn(function_handler)).await
}