pipeline {
    agent any

    environment {
        CARGO_HOME = "${WORKSPACE}/.cargo"
        RUSTUP_HOME = "${WORKSPACE}/.rustup"
        PATH = "${WORKSPACE}/.cargo/bin:${WORKSPACE}/bin:${WORKSPACE}/venv/bin:${WORKSPACE}/node/bin:${env.PATH}"
        PROTOC = "${WORKSPACE}/protoc/bin/protoc"
    }

    stages {
        stage('Checkout') {
            steps {
                checkout scm
            }
        }

        stage('Setup') {
            steps {
                sh 'curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable'
                sh 'bash activate.sh'
                sh 'cargo install --locked cargo-zigbuild'
                sh '''python3 -m venv ${WORKSPACE}/venv
                      ${WORKSPACE}/venv/bin/pip install --upgrade pip
                      ${WORKSPACE}/venv/bin/pip install ziglang'''
                sh 'curl --proto "=https" --tlsv1.2 -sSf https://just.systems/install.sh | bash -s -- --to ${WORKSPACE}/bin'
                sh 'curl -fsSL https://github.com/protocolbuffers/protobuf/releases/download/v28.3/protoc-28.3-linux-x86_64.zip -o /tmp/protoc.zip && unzip -o /tmp/protoc.zip -d ${WORKSPACE}/protoc && chmod +x ${WORKSPACE}/protoc/bin/protoc'
                sh 'mkdir -p ${WORKSPACE}/node && curl -fsSL https://nodejs.org/dist/v22.18.0/node-v22.18.0-linux-x64.tar.xz | tar -xJ -C ${WORKSPACE}/node --strip-components=1'
            }
        }

        stage('Lint') {
            parallel {
                stage('Format') {
                    steps {
                        sh 'just fmt && git diff --exit-code'
                    }
                }
                stage('Clippy') {
                    steps {
                        sh 'just lint'
                    }
                }
            }
        }

        stage('Test') {
            parallel {
                stage('Rust Tests') {
                    steps {
                        sh 'bash scripts/test2junit.sh > test-results.xml'
                        junit 'test-results.xml'
                    }
                }
                stage('WebUI Contract Test') {
                    steps {
                        sh 'just test-webui'
                    }
                }
            }
        }

        stage('Build') {
            parallel {
                stage('Build Host') {
                    steps {
                        sh 'cargo build --release -p swarmlink-host'
                    }
                }
                stage('Build Agent') {
                    steps {
                        sh 'cargo build --release -p swarmlink-agent'
                    }
                }
                stage('Build CLI') {
                    steps {
                        sh 'cargo build --release -p swarmlink-cli'
                    }
                }
            }
        }

        stage('Cross-Compile Agent') {
            steps {
                sh 'just cross-compile-arm'
                sh 'just cross-compile-armv7'
                sh 'just cross-compile-x86_64'
            }
        }
    }

    post {
        always {
            archiveArtifacts artifacts: 'test-results.xml, target/release/swarmlink-host, target/release/swarmlink-agent, target/release/swarmlink-cli, target/aarch64-unknown-linux-musl/release/swarmlink-agent, target/armv7-unknown-linux-musleabihf/release/swarmlink-agent, target/x86_64-unknown-linux-musl/release/swarmlink-agent', fingerprint: true, allowEmptyArchive: true
            cleanWs()
        }
    }
}
