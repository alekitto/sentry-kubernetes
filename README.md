sentry-kubernetes
=================

Sentry Kubernetes monitor in Rust. The original (pyhton) project can be found here: https://github.com/getsentry/sentry-kubernetes/

Errors and warnings in Kubernetes often go unnoticed by operators. Even when they are checked they are hard to read 
and understand in the context of what else is going on in the cluster. `sentry-kubernetes` is a small container you 
launch inside your Kubernetes cluster that will send errors and warnings to Sentry where they will be cleanly presented
and intelligently grouped. Typical Sentry features such as notifications can then be used to help operation and 
developer visibility.

Create a new project on [Sentry](http://sentry.io/) (or your self-hosted instance) and use your DSN when
launching the `sentry-kubernetes` container:

    kubectl run sentry-kubernetes \
      --image ghcr.io/alekitto/sentry-kubernetes \
      --env="DSN=$YOUR_DSN"

#### Filters and options

| ENV var                   | Description                                                                                                                                    |
|---------------------------|------------------------------------------------------------------------------------------------------------------------------------------------|
| EVENT_NAMESPACES          | A comma-separated list of namespaces to be included. If set, only the events from these namespace will be sent to Sentry.                      |
| EVENT_NAMESPACES_EXCLUDED | A comma-separated list of namespaces. Events from these namespaces won't be sent to Sentry.                                                    |
| COMPONENT_FILTER          | A comma-separated list of component names. Events from these components (ex: kubelet) won't be sent to Sentry.                                 |
| REASON_FILTER             | A comma-separated list of reasons (error codes). Events which have these reasons (ex: FailedMount) won't be sent to Sentry.                    |
| EVENT_LEVELS              | A comma-separated list of event levels (default: "warning,error"). Only events of these levels will be sent to Sentry. Errors are always sent. |

## Install using helm charts

```console
$ helm install oci://ghcr.io/alekitto/sentry-kubernetes-chart/sentry-kubernetes release-name --set sentry.dsn=<your-dsn>
```

See [charts README](./chart/README.md) for more information.
