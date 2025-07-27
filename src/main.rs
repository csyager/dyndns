use aws_config::{defaults, BehaviorVersion};
use aws_config::meta::region::RegionProviderChain;
use aws_sdk_route53::{config::Region, types::{HostedZone, RrType, ChangeBatch, ChangeAction, Change, ResourceRecord, ResourceRecordSet}, Client};

use clap::Parser;

use env_logger::{Builder, Env};
use log::{info, debug};

use reqwest;

use serde::Deserialize;

use std::io::{Error as ioError, ErrorKind};
use std::error::Error as Error;


const IP_SERVICE: &str = "http://httpbin.org/ip";

/// CLI options struct.
#[derive(Debug, Parser)]
struct Opt {
    /// The AWS Region.
    #[structopt(short, long)]
    region: Option<String>,

    /// The hosted zone domain to update
    #[structopt(short, long)]
    domain: String,

    /// Subdomain to update
    #[structopt(short, long)]
    subdomain: String
}

/// External IP address, as sourced from httpbin.org.
#[derive(Deserialize, Debug)]
struct ExternalIp {
    origin: String
}


/// Get the external IP of the current network.
async fn get_external_ip() -> Result<ExternalIp, Box<dyn Error>> {
    let ip = reqwest::get(IP_SERVICE)
        .await?
        .json::<ExternalIp>()
        .await?;
    info!("Got external IP address {}", ip.origin);
    Ok(ip)
}

/// Get HostedZone info from AWS Route53.
async fn parse_host_info(client: &aws_sdk_route53::Client) -> Result<Vec<HostedZone>, aws_sdk_route53::Error> {
    let hosted_zone_count = client.get_hosted_zone_count().send().await?;
    let mut hosted_zones_vec = Vec::new();

    info!(
        "Number of hosted zones in region : {}",
        hosted_zone_count.hosted_zone_count(),
    );

    let hosted_zones = client.list_hosted_zones().send().await?;

    info!("Zones:");

    for hz in hosted_zones.hosted_zones() {
        let zone_name = hz.name();
        let zone_id = hz.id();

        info!("  ID :   {}", zone_id);
        info!("  Name : {}", zone_name);

        hosted_zones_vec.push(hz.clone());
    }

    Ok(hosted_zones_vec)
}

/// Get HostedZone ID for a domain from HostedZone info.
fn get_hosted_zone_id(hosted_zones: &Vec<HostedZone>, domain: &str) -> Result<String, ioError> {
    for hosted_zone in hosted_zones {
        if hosted_zone.name().contains(domain) {
            return Ok(String::from(hosted_zone.id().split("/").nth(2).expect("Failed to parse hosted zone id.")));
        }
    }
    Err(ioError::new(ErrorKind::NotFound, "Hosted zone for domain not found"))
}

/// Checks a HostedZone's resource records for the fully-qualified domain name, and checks if the external IP matches the resource configuration.
async fn check_hosted_zone(client: &aws_sdk_route53::Client, hosted_zone_id: &str, external_ip: &str, domain: &str, subdomain: &str) -> Result<bool, Box<dyn Error>> {
    let full_domain = format!("{}.{}.", subdomain, domain);
    let request = client.list_resource_record_sets()
        .hosted_zone_id(hosted_zone_id)
        .start_record_name(&full_domain)
        .start_record_type(RrType::A);
    let response = request.send().await?;
    
    for resource_record_set in response.resource_record_sets {
        if resource_record_set.name == full_domain {
            let resource_records = resource_record_set.resource_records.expect(&format!("Domain {} did not contain any resource records.", full_domain));
            for resource_record in resource_records {
                if resource_record.value != external_ip {
                    info!("Resource record is out-of-date, and should be updated.");
                    return Ok(true);
                } else {
                    info!("Resource record is up-to-date, no update necessary.");
                    return Ok(false);
                }
            }
        }
    }
    Err(Box::new(ioError::new(ErrorKind::NotFound, format!("ResourceRecordSet for domain {} not found.", full_domain))))
}

/// Updates the HostedZone resource with the external IP address.
async fn update_hosted_zone(client: &aws_sdk_route53::Client, hosted_zone_id: &str, external_ip: &str, domain: &str, subdomain: &str) -> Result<(), Box<dyn Error>> {
    let full_domain = format!("{}.{}.", subdomain, domain);
    let request = client.change_resource_record_sets()
        .hosted_zone_id(hosted_zone_id)
        .change_batch(ChangeBatch::builder()
            .changes(Change::builder()
                .action(ChangeAction::Upsert)
                .resource_record_set(ResourceRecordSet::builder()
                    .name(full_domain)
                    .r#type(RrType::A)
                    .ttl(300)
                    .resource_records(ResourceRecord::builder()
                        .value(external_ip)
                        .build()?)
                    .build()?)
                .build()?)
            .build()?);
    debug!("Request: {:?}", request); 
    let response = request.send().await?;
    debug!("Response: {:?}", response);

    Ok(())

} 

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Configure logger
    let env = Env::default().filter_or("RUST_LOG", "info"); //
    Builder::from_env(env).init();

    // configure AWS client
    let Opt { region, domain, subdomain } = Opt::parse();

    let region_provider = RegionProviderChain::first_try(region.map(Region::new))
        .or_default_provider()
        .or_else(Region::new("us-east-1"));
    let shared_config = defaults(BehaviorVersion::latest())
        .region(region_provider)
        .load()
        .await;
    let client = Client::new(&shared_config);

    let external_ip = get_external_ip().await?;
    let hosted_zones: Vec<HostedZone> = parse_host_info(&client).await?;
    
    let hosted_zone_id = get_hosted_zone_id(&hosted_zones, &domain)?;
    info!("Hosted zone id: {}", hosted_zone_id);

    let needs_update = check_hosted_zone(&client, &hosted_zone_id, &external_ip.origin, &domain, &subdomain).await?;

    if needs_update {
        update_hosted_zone(&client, &hosted_zone_id, &external_ip.origin, &domain, &subdomain).await?
    }

    Ok(())
}
