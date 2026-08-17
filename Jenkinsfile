pipeline {
    agent any

    environment {
        PIXI_HOME = "${WORKSPACE}/.pixi"
        CARGO_HOME = "${WORKSPACE}/.cargo"
        RUSTUP_HOME = "${WORKSPACE}/.rustup"
        PATH = "${WORKSPACE}/.cargo/bin:${WORKSPACE}/.pixi/bin:${env.PATH}"
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
                sh 'pixi install'
                sh 'echo "export PROTOC=$(pixi run which protoc | tr -d[:space:])" >> ${WORKSPACE}/.cargo/env'
            }
        }

        stage('Lint') {
            parallel {
                stage('Format') {
                    steps {
                        sh '. ${WORKSPACE}/.cargo/env && pixi run fmt && git diff --exit-code'
                    }
                }
                stage('Clippy') {
                    steps {
                        sh '. ${WORKSPACE}/.cargo/env && pixi run lint'
                    }
                }
            }
        }

        stage('Test') {
            parallel {
                stage('Rust Tests') {
                    steps {
                        sh '. ${WORKSPACE}/.cargo/env && pixi run test-rust'
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
                        sh '. ${WORKSPACE}/.cargo/env && cargo build --release -p swarmdeck-host'
                    }
                }
                stage('Build Agent') {
                    steps {
                        sh '. ${WORKSPACE}/.cargo/env && cargo build --release -p swarmdeck-agent'
                    }
                }
                stage('Build CLI') {
                    steps {
                        sh '. ${WORKSPACE}/.cargo/env && cargo build --release -p swarmdeck-cli'
                    }
                }
            }
        }

        stage('Cross-Compile Agent') {
            parallel {
                stage('Agent aarch64') {
                    steps {
                        sh '. ${WORKSPACE}/.cargo/env && pixi run agent-aarch64'
                    }
                }
                stage('Agent armv7') {
                    steps {
                        sh '. ${WORKSPACE}/.cargo/env && pixi run agent-armv7'
                    }
                }
                stage('Agent x86_64') {
                    steps {
                        sh '. ${WORKSPACE}/.cargo/env && pixi run agent-x86_64'
                    }
                }
            }
        }
    }

    post {
        success {
            archiveArtifacts artifacts: 'target/release/swarmdeck-host, target/release/swarmdeck-agent, target/release/swarmdeck-cli, target/aarch64-unknown-linux-musl/release/swarmdeck-agent, target/armv7-unknown-linux-musleabihf/release/swarmdeck-agent, target/x86_64-unknown-linux-musl/release/swarmdeck-agent', fingerprint: true
        }
        always {
            cleanWs()
        }
    }
}
