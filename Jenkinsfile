pipeline {
    agent any

    environment {
        PIXI_HOME = "${WORKSPACE}/.pixi"
        PATH = "${WORKSPACE}/.pixi/bin:${env.PATH}"
    }

    stages {
        stage('Checkout') {
            steps {
                checkout scm
            }
        }

        stage('Setup') {
            steps {
                sh 'curl -fsSL https://pixi.sh/install.sh | bash'
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
                        sh 'pixi run test-rust'
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
        success {
            archiveArtifacts artifacts: 'target/release/swarmdeck-host, target/release/swarmdeck-agent, target/release/swarmdeck-cli, target/aarch64-unknown-linux-musl/release/swarmdeck-agent, target/armv7-unknown-linux-musleabihf/release/swarmdeck-agent, target/x86_64-unknown-linux-musl/release/swarmdeck-agent', fingerprint: true
        }
        always {
            cleanWs()
        }
    }
}
