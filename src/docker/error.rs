use std::fmt;

#[derive(Debug)]
pub enum DockerError {
    NotInstalled,
    DaemonNotRunning,
    PermissionDenied,
    ContainerNotFound(String),
    ContainerAlreadyExists(String),
    ImageNotFound(String),
    CreateFailed(String),
    StartFailed(String),
    StopFailed(String),
    RemoveFailed(String),
    CommandFailed(String),
    IoError(std::io::Error),
}

impl fmt::Display for DockerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DockerError::NotInstalled => write!(
                f,
                "Docker is not installed or not in PATH.\n\
                 Install Docker: https://docs.docker.com/get-docker/"
            ),
            DockerError::DaemonNotRunning => write!(
                f,
                "Docker daemon is not running.\n\
                 Start Docker Desktop or run: sudo systemctl start docker"
            ),
            DockerError::PermissionDenied => write!(
                f,
                "Docker permission denied.\n\
                 On Linux, add your user to the docker group:\n\
                 sudo usermod -aG docker $USER\n\
                 Then log out and back in."
            ),
            DockerError::ContainerNotFound(name) => {
                write!(f, "Container not found: {}", name)
            }
            DockerError::ContainerAlreadyExists(name) => {
                write!(f, "Container already exists: {}", name)
            }
            DockerError::ImageNotFound(image) => {
                write!(f, "Docker image not found: {}", image)
            }
            DockerError::CreateFailed(msg) => write!(f, "Failed to create container: {}", msg),
            DockerError::StartFailed(msg) => write!(f, "Failed to start container: {}", msg),
            DockerError::StopFailed(msg) => write!(f, "Failed to stop container: {}", msg),
            DockerError::RemoveFailed(msg) => write!(f, "Failed to remove container: {}", msg),
            DockerError::CommandFailed(msg) => write!(f, "Docker command failed: {}", msg),
            DockerError::IoError(err) => write!(f, "IO error: {}", err),
        }
    }
}

impl std::error::Error for DockerError {}

impl From<std::io::Error> for DockerError {
    fn from(err: std::io::Error) -> Self {
        DockerError::IoError(err)
    }
}

pub type Result<T> = std::result::Result<T, DockerError>;
