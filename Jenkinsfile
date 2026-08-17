pipeline {
    agent any

    environment {
        CARGO_HOME = "${WORKSPACE}/.cargo"
        RUSTUP_HOME = "${WORKSPACE}/.rustup"
        RUSTUP_INIT_ROOT = "${WORKSPACE}/.rustup"
    }

    tools {
        rustup 'rust-stable'
    }

    stages {
        stage('Setup') {
            steps {
                sh 'rustup target add aarch64-unknown-linux-musl armv7-unknown-linux-musleabihf x86_64-unknown-linux-musl'
                sh 'rustup component add clippy rustfmt'
            }
        }

        stage('Lint') {
            parallel {
                stage('Format') {
                    steps {
                        sh 'cargo fmt --all -- --check'
                    }
                }
                stage('Clippy') {
                    steps {
                        sh 'cargo clippy --workspace --all-targets -- -D warnings'
                    }
                }
            }
        }

        stage('Test') {
            parallel {
                stage('Rust Tests') {
                    steps {
                        sh 'cargo test --workspace'
                    }
                }
                stage('WebUI Contract Test') {
                    steps {
                        sh 'node ui/test/contract.test.js'
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
                        sh 'cargo zigbuild -p swarmdeck-agent --release --target aarch64-unknown-linux-musl'
                    }
                }
                stage('Agent armv7') {
                    steps {
                        sh 'cargo zigbuild -p swarmdeck-agent --release --target armv7-unknown-linux-musleabihf'
                    }
                }
                stage('Agent x86_64') {
                    steps {
                        sh 'cargo zigbuild -p swarmdeck-agent --release --target x86_64-unknown-linux-musl'
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
