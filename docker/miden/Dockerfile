ARG BASE_IMAGE_TAG=ere-base:latest

FROM ${BASE_IMAGE_TAG}

# The ere-base image provides Rust, Cargo, and common tools.
# We operate as root for SDK installation.

# Copy the Miden SDK installer script from the workspace context
COPY scripts/sdk_installers/install_miden_sdk.sh /tmp/install_miden_sdk.sh
RUN chmod +x /tmp/install_miden_sdk.sh

# Run the Miden SDK installation script.
RUN /bin/bash ./scripts/sdk_installers/install_miden_sdk.sh

# Verify Miden installation by checking that ere-miden crate builds
RUN echo "Verifying Miden installation in Dockerfile (post-script)..." && (cargo +stable check -p ere-miden || cargo check -p ere-miden)

CMD ["/bin/bash"]
