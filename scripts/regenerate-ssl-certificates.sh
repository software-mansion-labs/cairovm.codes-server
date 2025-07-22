echo "$(date '+%Y-%m-%d %H:%M:%S') Starting regenerating SSL certificate for api2.cairovm.codes"
sudo certbot certonly --standalone -d api2.cairovm.codes --staple-ocsp -m roman@walnut.dev --agree-tos --non-interactive --no-eff-email
echo "$(date '+%Y-%m-%d %H:%M:%S') Finished regenerating SSL certificate for api2.cairovm.codes"