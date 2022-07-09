# epok

epok is an external port operator for self-hosted kubernetes clusters. Define a 
`getbetter.ro/externalport: "25:2025"` annotation on your service and epok
will handle the `iptables` rules to forward port `25` on your host machine to
[NodePort](https://kubernetes.io/docs/concepts/services-networking/service/#publishing-services-service-types) `2025` on your kube nodes.

## Usage

```
epok --interface <INTERFACE> <SUBCOMMAND>

OPTIONS:
    -i, --interface <INTERFACE>    Interface to forward packets from [env: EPOK_INTERFACE=]

SUBCOMMANDS:
    local    Run operator on bare metal host
    ssh      Run operator inside cluster, SSH-ing back to the metal
```

### SSH Mode

This is a hack for allowing epok deployed in a self-hosted cluster to talk
to the host machine and manipulate its `iptables` rules.

It's best to use a separate account for this:

```shell
# create the user
$ sudo useradd --create-home epok; sudo passwd -d epok

# create and authorize an SSH key
$ sudo su epok -c ssh-keygen
$ sudo mv /home/epok/.ssh/id_rsa.pub /home/epok/.ssh/authorized_keys 

# restrict the epok user to iptables + iptables-save commands
$ echo '%epok ALL=(ALL) NOPASSWD: /usr/sbin/iptables, /usr/sbin/iptables-save' | sudo EDITOR='tee' VISUAL='tee' visudo -f /etc/sudoers.d/epok
```

You'll also need to copy the generated private key to a safe place.
[`sealedsecrets`](https://github.com/bitnami-labs/sealed-secrets) is a good solution for storing it inside the cluster.

To run epok:

```
$ epok -i eth0 ssh -H epok@<host_ip> -k /path/to/private.key
```

### Kubernetes deployment example

Let's say your Kubernetes nodes can reach the host machine on the using the 
`10.0.0.1` IP address; ssh is running on port `22222` and you saved the 
ssh private key to `/path/to/private.key`. The interface you want to route
from is `eth0`.

First, we need a namespace:

```shell
$ kubectl create ns epok
```

Store the SSH connection data as a secret:

```shell
$ kubectl create secret generic epok-ssh \
  --from-literal=ssh_host=10.0.0.1 \
  --from-literal=ssh_port=22222 \ 
  --from-literal=id_rsa="$(cat /path/to/private.key)"
```

Deploy epok using the supplied [example manifests](examples/k8s-manifests.yaml):

```shell
$ EPOK_INTERFACE="eth0" envsubst < examples/k8s-manifests.yaml | kubectl apply -f -
```
