# CIEL RPC Protocol Specification [Draft]

This RFC describes the basic CIEL RPC protocols.

WARNING: It is still subject to change without notice at this stage.

## Transport

CIEL RPC should use UNIX sockets for communication. The client application needs to have root permissions to access this socket.

CIEL RPC should use `bincode` format as its on-wire protocol format for performance and compatibility.

## Authentication

No authentication is performed on the connection to keep CIEL's design simple. The client application may need to handle the authentication if necessary.

## Commands and Responses

TODO
