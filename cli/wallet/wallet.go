package wallet

import (
	"fmt"
	"sync"

	flag "github.com/spf13/pflag"

	coreSignature "github.com/oasisprotocol/oasis-core/go/common/crypto/signature"
	"github.com/oasisprotocol/oasis-sdk/client-sdk/go/crypto/signature"
	"github.com/oasisprotocol/oasis-sdk/client-sdk/go/types"
)

var registeredFactories sync.Map

// WalletFactory is a factory that supports wallets of a specific kind.
type WalletFactory interface {
	// Kind returns the kind of wallets this factory will produce.
	Kind() string

	// Flags returns the CLI flags that can be used for configuring this wallet factory.
	Flags() *flag.FlagSet

	// GetConfigFromFlags generates wallet configuration from flags.
	GetConfigFromFlags() (map[string]interface{}, error)

	// Create creates a new wallet.
	Create(name string, passphrase string, cfg map[string]interface{}) (Wallet, error)

	// Load loads an existing wallet.
	Load(name string, passphrase string, cfg map[string]interface{}) (Wallet, error)

	// Remove removes an existing wallet.
	Remove(name string, cfg map[string]interface{}) error
}

// Wallet is the wallet interface.
type Wallet interface {
	// ConsensusSigner returns the consensus layer signer associated with the wallet.
	//
	// It may return nil in case this wallet cannot be used with the consensus layer.
	ConsensusSigner() coreSignature.Signer

	// Signer returns the signer associated with the wallet.
	Signer() signature.Signer

	// Address returns the address associated with the wallet.
	Address() types.Address

	// SignatureAddressSpec returns the signature address specification associated with the wallet.
	SignatureAddressSpec() types.SignatureAddressSpec

	// UnsafeExport exports the wallet's secret state.
	UnsafeExport() string
}

// Register registers a new wallet type.
func Register(wf WalletFactory) {
	if _, loaded := registeredFactories.LoadOrStore(wf.Kind(), wf); loaded {
		panic(fmt.Sprintf("wallet: kind '%s' is already registered", wf.Kind()))
	}
}

// Load loads a previously registered wallet factory.
func Load(kind string) (WalletFactory, error) {
	wf, loaded := registeredFactories.Load(kind)
	if !loaded {
		return nil, fmt.Errorf("wallet: kind '%s' not available", kind)
	}
	return wf.(WalletFactory), nil
}

// AvailableKinds returns all of the available wallet factories.
func AvailableKinds() []WalletFactory {
	var kinds []WalletFactory
	registeredFactories.Range(func(key, value interface{}) bool {
		kinds = append(kinds, value.(WalletFactory))
		return true
	})
	return kinds
}
