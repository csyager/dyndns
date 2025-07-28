# dyndns

Short script written in Rust to dynamically update a Route53 record with the host's current public IP address.

The purpose for this script is to be run periodically from a host with a non-static IP address (for example, if your ISP is like mine and frequently assigns new IP addresses to your router).

## How to run it

dyndns --domain [hosted zone domain] --subdomain [subdomain for A record to update]

## How to use it

First, you'll need a Route53 hosted zone.  In your hosted zone, create an A record for your desired subdomain.  This record will point to the public IP address of your network.  Running this script from a host on your network with valid AWS credentials will update the A record to point at your public IP address.

I've configured a systemd executable to run this script periodically, like so:

`dyndns.service`

```
[Unit]
Description=Hourly job to dynamically update DNS configuration

[Service]
ExecStart=dyndns --domain [domain] --subdomain [subdomain]
```

`dyndns.timer`

```
[Unit]
Description=Run the hourly dynamic DNS update

[Timer]
OnCalendar=hourly

[Install]
WantedBy=timers.target
```

This will instruct systemd to execute the dyndns script at the top-of-the-hour.
