pipeline {
    agent any

    environment {
        PIXI_HOME = "${WORKSPACE}/.pixi"
        CARGO_HOME = "${WORKSPACE}/.cargo"
        RUSTUP_HOME = "${WORKSPACE}/.rustup"
        PATH = "${WORKSPACE}/.cargo/bin:${WORKSPACE}/.pixi/bin:${WORKSPACE}/node/bin:${env.PATH}"
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
                sh 'curl -fsSL https://pixi.sh/install.sh | bash'
                sh 'curl -fsSL https://github.com/protocolbuffers/protobuf/releases/download/v28.3/protoc-28.3-linux-x86_64.zip -o /tmp/protoc.zip && unzip -o /tmp/protoc.zip -d ${WORKSPACE}/protoc && chmod +x ${WORKSPACE}/protoc/bin/protoc'
                sh 'mkdir -p ${WORKSPACE}/node && curl -fsSL https://nodejs.org/dist/v22.18.0/node-v22.18.0-linux-x64.tar.xz | tar -xJ -C ${WORKSPACE}/node --strip-components=1'
                sh 'pixi install'
            }
        }

        stage('Lint') {
            parallel {
                stage('Format') {
                    steps {
                        sh 'pixi run fmt && git diff --exit-code'
                    }
                }
                stage('Clippy') {
                    steps {
                        sh 'pixi run lint'
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
                        sh 'pixi run test-webui'
                    }
                }
            }
        }

        stage('Build') {
            parallel {
                stage('Build Host') {
                    steps {
                        sh 'cargo build --release -p swarmdeck-host'
                    }
                }
                stage('Build Agent') {
                    steps {
                        sh 'cargo build --release -p swarmdeck-agent'
                    }
                }
                stage('Build CLI') {
                    steps {
                        sh 'cargo build --release -p swarmdeck-cli'
                    }
                }
            }
        }

        stage('Cross-Compile Agent') {
            parallel {
                stage('Agent aarch64') {
                    steps {
                        sh 'pixi run agent-aarch64'
                    }
                }
                stage('Agent armv7') {
                    steps {
                        sh 'pixi run agent-armv7'
                    }
                }
                stage('Agent x86_64') {
                    steps {
                        sh 'pixi run agent-x86_64'
                    }
                }
            }
        }
    }

    post {
        always {
            archiveArtifacts artifacts: 'test-results.xml, target/release/swarmdeck-host, target/release/swarmdeck-agent, target/release/swarmdeck-cli, target/aarch64-unknown-linux-musl/release/swarmdeck-agent, target/armv7-unknown-linux-musleabihf/release/swarmdeck-agent, target/x86_64-unknown-linux-musl/release/swarmdeck-agent', fingerprint: true, allowEmptyArchive: true
            cleanWs()
        }
    }
}
