# epok

epok is an external port operator for self-hosted kubernetes clusters. Define a 
`getbetter.ro/externalport: "25:2025"` annotation on your service and epok
will handle the `iptables` rules to forward port `25` on your host machine to
[NodePort](https://kubernetes.io/docs/concepts/services-networking/service/#publishing-services-service-types) `2025` on your kube nodes.

## Usage

```
epok [OPTIONS] --interfaces <INTERFACES> <EXECUTOR>

OPTIONS:
    -i, --interfaces <INTERFACES>
            Comma-separated list of interfaces to forward packets from [env: EPOK_INTERFACES=]

        --batch-commands <batch-commands>
            Batch the execution of iptables commands [env: EPOK_BATCH_COMMANDS=] [default: true]

        --batch-size <BATCH_SIZE>
            Maximum command batch size [env: EPOK_BATCH_SIZE=] [default: 1677722]

    -h, --help
            Print help information

    -V, --version
            Print version information

EXECUTORS:
    local    Execute commands locally
    ssh      Execute commands through ssh
```

### SSH Executor

This is a hack for allowing epok deployed in a self-hosted cluster to talk
to the host machine and manipulate its `iptables` rules.

It's best to use a separate account for this:

```shell
export EPOK_USER=epok

# create the user
sudo useradd --create-home $EPOK_USER; sudo passwd -d $EPOK_USER

# create and authorize an SSH key
sudo su $EPOK_USER -c ssh-keygen
sudo mv /home/$EPOK_USER/.ssh/id_rsa.pub /home/$EPOK_USER/.ssh/authorized_keys 

# grab the private key and store it in a safe place
sudo mv /home/$EPOK_USER/.ssh/id_rsa /path/to/private.key

# restrict the epok user to iptables + iptables-save commands
echo "%${EPOK_USER} ALL=(ALL) NOPASSWD: /usr/sbin/iptables, /usr/sbin/iptables-save" | sudo EDITOR='tee' VISUAL='tee' visudo -f /etc/sudoers.d/$EPOK_USER
```

NOTE: [`sealedsecrets`](https://github.com/bitnami-labs/sealed-secrets) is a good solution for storing the private key inside the cluster.

To test epok connectivity:

```shell
epok -i eth0 ssh --host $EPOK_USER@host_machine --key /path/to/private.key
```

### Kubernetes deployment example

Set up some configuration values:

```shell
cat > epok.config <<EOF
# Where should we push the docker image? Should be reachable from cluster.
EPOK_IMAGE="my.docker.registry/epok:latest"

# What interfaces should epok forward packets from?
EPOK_INTERFACES="eth0"

# What user@host should epok use to contact the host machine?
EPOK_SSH_HOST=user@10.0.0.1

# On what port is sshd running on the host?
EPOK_SSH_PORT=22222

# What key should we use to authenticate?
EPOK_SSH_KEY=/path/to/private.key

# What namespace shall we deploy to?
EPOK_NS=epok

export EPOK_IMAGE EPOK_INTERFACE EPOK_SSH_HOST EPOK_SSH_PORT EPOK_SSH_KEY EPOK_NS
EOF
```

Dockerize epok:

```shell
source epok.config
docker build -f docker/Dockerfile . -t $EPOK_IMAGE
docker push $EPOK_IMAGE
```

Create the namespace, secret and deploy using the supplied [example manifests](examples/k8s-manifests.yaml):

```shell
source epok.config
kubectl create ns $EPOK_NS
kubectl create secret -n $EPOK_NS generic epok-ssh \
  --from-file=id_rsa=$EPOK_SSH_KEY \
  --from-literal=ssh_host=$EPOK_SSH_HOST \
  --from-literal=ssh_port=$EPOK_SSH_PORT
envsubst < examples/k8s-manifests.yaml | kubectl apply -f -
```
