module github.com/oasisprotocol/oasis-sdk/client-sdk/go

go 1.16

// Should be synced with Oasis Core as replace directives are not propagated.
replace (
	github.com/tendermint/tendermint => github.com/oasisprotocol/tendermint v0.34.9-oasis2
	golang.org/x/crypto/curve25519 => github.com/oasisprotocol/curve25519-voi/primitives/x25519 v0.0.0-20210505121811-294cf0fbfb43
	golang.org/x/crypto/ed25519 => github.com/oasisprotocol/curve25519-voi/primitives/ed25519 v0.0.0-20210505121811-294cf0fbfb43
)

require (
	github.com/btcsuite/btcd v0.22.0-beta
	github.com/golang/snappy v0.0.4
	github.com/oasisprotocol/curve25519-voi v0.0.0-20210716083614-f38f8e8b0b84
	github.com/oasisprotocol/deoxysii v0.0.0-20200527154044-851aec403956
	github.com/oasisprotocol/oasis-core/go v0.2103.6
	github.com/stretchr/testify v1.8.1
	golang.org/x/crypto v0.0.0-20211108221036-ceb1ce70b4fa
	google.golang.org/grpc v1.53.0
)
