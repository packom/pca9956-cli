// https://www.jenkins.io/doc/book/pipeline/docker/
pipeline {
    agent {
        docker { image 'piersfinlayson/build-amd64:0.3.2' }
    }
    stages {
        stage('Clone') {
            steps {
                withCredentials([usernamePassword(credentialsId: 'github.packom', usernameVariable: 'USERNAME', passwordVariable: 'PASSWORD')]) {
                    sh '''
                        cd ~/builds && \
                        git clone https://packom:$PASSWORD@github.com/packom/pca9956b-cli && \
                        cd pca9956b-cli && \
                        echo `awk '/^version / {print $3;}' Cargo.toml | sed 's/"//g'` > /tmp/version && \
                        echo "Current version is:" && \
                        cat /tmp/version
                    '''
                }
            }
        }
        stage('Build') {
            steps {
                sh '''
                    cd ~/builds/pca9956b-cli && \
                    cargo build
                '''
            }
        }
        stage('Test') {
            steps {
                sh '''
                    cd ~/builds/pca9956b-cli && \
                    cargo test
                '''
            }
        }
    }
}
