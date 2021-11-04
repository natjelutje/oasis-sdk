package core

import (
	"github.com/oasisprotocol/oasis-sdk/client-sdk/go/types"
)

// EstimateGasQuery is the body of the core.EstimateGas query.
type EstimateGasQuery struct {
	// Caller is the address of the caller for which to do estimation. If not specified the
	// authentication information from the passed transaction is used.
	Caller *CallerAddress `json:"caller,omitempty"`
	// Tx is the unsigned transaction to estimate.
	Tx *types.Transaction `json:"tx"`
}

// CallerAddress is a caller for the EstimateGasQuery.
type CallerAddress struct {
	// Address is an oasis address.
	Address *types.Address `json:"address,omitempty"`
	// EthAddress is an ethereum address.
	EthAddress [20]byte `json:"eth_address,omitempty"`
}
