# Deployment Runbook

## 1) Overview

This runbook deploys two combined images:

- `gobgp-stunnel-tcpao` (initiator side)
- `gobmp-tls-tcpao` (terminator side)

Each role is a single container that runs both:

- application process (`gobgp-stunnel` or `gobmp-tls`)
- `tcpao-proxy` process (initiator or terminator mode)

Wire leg between these two combined containers uses TCP-AO.

## 2) Host preflight

Run on the Linux host where containers will run:

```bash
./scripts/doctor.sh
grep -E '^CONFIG_TCP_AO=' /boot/config-$(uname -r)
```

If the host is AO-capable and privileged enough, strict functional test should pass:

```bash
make test-functional-strict
```

## 3) Build images

Build terminator image:

```bash
docker build \
  -f deploy/images/gobmp-tls-tcpao/Dockerfile \
  -t ghcr.io/<org>/gobmp-tls-tcpao:dev .
```

Build initiator image:

```bash
docker build \
  -f deploy/images/gobgp-stunnel-tcpao/Dockerfile \
  -t ghcr.io/<org>/gobgp-stunnel-tcpao:dev .
```

Optional base-image override:

```bash
docker build \
  --build-arg BASE_IMAGE=ghcr.io/asadarafat/gobmp:v1.0.4-alpha-tls \
  -f deploy/images/gobmp-tls-tcpao/Dockerfile \
  -t ghcr.io/<org>/gobmp-tls-tcpao:dev .
```

## 4) Required runtime env

Both containers require:

- `TCPAO_KEY`: shared AO key (same value on both sides)
- `KEY_ID`: key id (default `1`)
- `PEER_IP`: remote sidecar wire IP
- `PEER_PORT`: remote sidecar wire port (default `1790`)

Initiator-specific:

- `REMOTE_AO`: `<terminator-ip>:<port>`
- `LISTEN_PLAIN`: local app-to-sidecar plain listen (default `127.0.0.1:5000`)

Terminator-specific:

- `LISTEN_AO`: AO listen endpoint (default `0.0.0.0:1790`)
- `FORWARD_PLAIN`: local sidecar-to-app plain endpoint (default `127.0.0.1:11019`)

Optional:

- `APP_CMD`: app startup command inside the image.

Notes:

- In the provided topology, `APP_CMD` is set to `sleep infinity` by default to keep containers stable.
- Replace `APP_CMD` with your real `gobgp-stunnel` / `gobmp-tls` startup command for full end-to-end app traffic.

## 5) containerlab deployment

Use provided topology:

```bash
containerlab deploy -t deploy/containerlab/tcpao-bmp.clab.yml
containerlab inspect -t deploy/containerlab/tcpao-bmp.clab.yml
```

Initial file uses placeholder key values; update `TCPAO_KEY` first.

Destroy lab:

```bash
containerlab destroy -t deploy/containerlab/tcpao-bmp.clab.yml
```

## 6) Runtime verification

Check proxy logs:

```bash
docker logs <initiator-container>
docker logs <terminator-container>
```

Expected indicators:

- terminator: `configured tcp-ao policies on listener`
- initiator: `applied outbound tcp-ao policy`
- data path: connection close logs with byte counters

Socket checks:

```bash
ss -ltnp | grep -E '1790|5000|11019'
```

Automated end-to-end AO + forwarding validation:

```bash
make test-validation-tcpao-proxy
```

Strict bidirectional data validation (both `from-goBGP-to-goBMP` and `from-goBMP-to-goBGP`):

```bash
REQUIRE_BIDIRECTIONAL_TRAFFIC=1 make test-validation-tcpao-proxy
```

Route-based validation via goBGP -> BMP -> goBMP:

```bash
make test-validation-tcpao-proxy-bgp-route
```

Notes:

- This target runs `containerlab deploy -t deploy/containerlab/tcpao-bmp.clab.yml --reconfigure`
- It injects payload through the initiator sidecar and validates AO/traffic evidence in both container logs
- With `REQUIRE_BIDIRECTIONAL_TRAFFIC=1`, backend mode defaults to `echo` (`BACKEND_MODE=auto`) so reverse-direction bytes are required
- It also prints goBGP/goBMP runtime config context (sidecar config, app config candidates, and process command lines) from both containers
- It prints a `traffic injection plan` section that explains the injection method and expected direction checks for the current mode
- Docker/containerlab privileges are required (`sudo -E` may be needed)
- Route-based target injects `ROUTE_PREFIX` (default `203.0.113.0/24`) into goBGP and verifies that prefix appears in goBMP dump output
- Route evidence is decoded and printed as pretty JSON using `jq`; if `jq` is missing the script attempts auto-install and falls back to non-pretty output if install fails
- The `make test-validation-tcpao-proxy-bgp-route` target uses `MAX_WAIT_SECS=30` by default

## 7) Negative test (fail closed)

Set different `TCPAO_KEY` values on each side and restart. Expected:

- wire connection fails
- no end-to-end payload delivery
- explicit AO-related error in logs

## 8) CI publishing

Use workflow dispatch:

- `.github/workflows/publish_gobmp_tls_tcpao.yml`
- `.github/workflows/publish_gobgp_stunnel_tcpao.yml`

Each workflow accepts `image_tag` and optional base image override.
