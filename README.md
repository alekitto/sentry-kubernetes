# sentry-kubernetes

![GitHub license](https://img.shields.io/github/license/alekitto/sentry-kubernetes)
![GitHub stars](https://img.shields.io/github/stars/alekitto/sentry-kubernetes)
![GitHub forks](https://img.shields.io/github/forks/alekitto/sentry-kubernetes)

`sentry-kubernetes` is an open-source tool designed to monitor a Kubernetes cluster and send notifications to [Sentry](https://sentry.io/) (SaaS or self-hosted) when errors are detected. This tool helps track critical events, improving incident management and operational visibility.

## Features
- **Real-time monitoring** of Kubernetes events.
- Integration with **Sentry** for error notifications.
- Support for both SaaS and self-hosted Sentry environments.
- Simple and flexible configuration.

## Requirements
- Kubernetes cluster (version >= 1.26)
- A configured project on [Sentry](https://sentry.io/) (SaaS or self-hosted).

## Installation

You can install `sentry-kubernetes` using Helm:

```bash
helm install sentry-kubernetes oci://ghcr.io/alekitto/sentry-kubernetes-chart/sentry-kubernetes
```

Ensure you configure the OCI repository before installation.

## Configuration

`sentry-kubernetes` is configured using a YAML file. Below is an example configuration structure:

```yaml
dsn: <string> # Global Sentry DSN
levels: # Severity levels to monitor
  - trace
  - debug
  - info
  - warning
  - error
log_level: <string> # Application log level (e.g., "info")
environment: <string> # Environment name (e.g., "production")
release: <string> # Application release/version
historical: <bool> # Send historical events (true or false)
monitors: # Monitor configuration
  - dsn: <string> # Specific DSN for the monitor (optional, uses global DSN if absent)
    environment: <string> # Specific environment (optional, uses global environment if absent)
    release: <string> # Specific release (optional, uses global release if absent)
    levels: # Specific levels for the monitor
      - warning
      - error
    resources: # List of resources to monitor (monitor all the resources if absent)
      - api_version: <string> # API version of the resource (e.g., "v1")
        kind: <string> # Resource type (e.g., "Pod", "Deployment")
        label_selector: <string> # Label selector (e.g., "app=my-app")
        namespace: <string> # Resource namespace
```

### Usage Example

```bash
helm install sentry-kubernetes oci://ghcr.io/alekitto/sentry-kubernetes-chart/sentry-kubernetes -f values.yaml
```

## Contributing

Contributions are welcome! Feel free to open issues or pull requests on [GitHub](https://github.com/alekitto/sentry-kubernetes).

1. Fork the repository.
2. Create a branch for your changes: `git checkout -b feature/my-feature`.
3. Make your changes.
4. Open a pull request.

## License

This project is licensed under the [MIT](LICENSE).

## Contact

If you have questions or feedback, feel free to open an issue or contact me directly on [GitHub](https://github.com/alekitto).
