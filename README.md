# DeRouter

[DeRouter](https://derouter.org) is a decentralized digital service marketplace.

## 📚 Overview

This repository is a DeRouter P2P node implementation.
It connects to other nodes in a decentralized manner.

As a DeRouter network user, you may be either Consumer or Provider, or both.
Consumers consume digital services provided by Providers.
Providers inform the network about the services they provide, and wait for direct connections from Consumers.

DeRouter is designed to be protocol-agnostic.
A Provider publishes some digital service offers, which include `"protocol"` and `"protocol_payload"` fields.
For example, some `"openai"` protocol may require provided models to be listed in the offer's payload, along with their context sizes, and pricing.
Providers use provider module implementations which connect to their local DeRouter node and contain actual protocol logic.
For example, the [DeRouter's OpenAI provider module](https://github.com/derouter/provider-openai) provides a simple solution to proxy incoming requests to an OpenAI-compatible URL.

Consumers see the actual list of provided offers in the network, and may choose to schedule a job for a specific offer.
Similar to Provider, Consumer module implementations are connected to a running DeRouter node, containing logic for service interaction.
For example, the [DeRouter's OpenAI consumer module](https://github.com/derouter/consumer-openai) exposes an OpenAI-compatible endpoint locally, proxying requests to the DeRouter network, finding the best matching offers according to some criteria, such as model ID, context size, and price limits.

Consumers will be able to pay for the provided services with cryptocurrency.

### ✅ Current State

DeRouter is currently in alpha.

DeRouter P2P node is implemented (this repository). It connects to the decentralized network via [mDNS](https://docs.libp2p.io/concepts/discovery-routing/mdns/) and bootstrap nodes, exposing a [CBOR](https://cbor.io/) RPC over TCP.

[OpenAI protocol](https://github.com/derouter/protocol-openai) along with [OpenAI provider](https://github.com/derouter/provider-openai) and [OpenAI consumer](https://github.com/derouter/consumer-openai) modules are implemented.
A module may use an RPC package for their language, such as [derouter/rpc-js](https://github.com/derouter/rpc-js).

You can help shaping the future of DeRouter by joining our communities:

- [Reddit](https://www.reddit.com/r/derouter)
- [Discord](https://discord.gg/vRuWUzfRpW)

### 🚧 Roadmap

- [ ] Cryptocurrency payments (Ethereum is being developed locally).
- [ ] Desktop application to explore offers & manage modules (Tauri app is being developed locally).

## 👷 Development

You must have Rust installed locally.

```sh
RUST_LOG=debug cargo run -- -c./config.json
```

Alternatively, you can download prebuilt binaries from the [Releases](https://github.com/derouter/derouter/releases) page.

## 🚀 Bootstrap

You would need to specify addresses of other DeRouter nodes connected to the network for peer discovery in your `config.json`.
These are some well-known relay nodes:

```json
{
  "bootstrap": [
    "/ip4/5.75.174.15/tcp/90/p2p/12D3KooWPHcqLQHQK3Rf7nCTMCAzM6QjA44UsrjAjhnYnokZzzJE"
  ]
}
```
