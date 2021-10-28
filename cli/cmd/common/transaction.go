package common

import (
	"context"
	"encoding/json"
	"fmt"

	"github.com/spf13/cobra"
	flag "github.com/spf13/pflag"

	coreSignature "github.com/oasisprotocol/oasis-core/go/common/crypto/signature"
	consensus "github.com/oasisprotocol/oasis-core/go/consensus/api"
	consensusTx "github.com/oasisprotocol/oasis-core/go/consensus/api/transaction"

	"github.com/oasisprotocol/oasis-sdk/cli/client"
	"github.com/oasisprotocol/oasis-sdk/cli/config"
	"github.com/oasisprotocol/oasis-sdk/cli/wallet"
	"github.com/oasisprotocol/oasis-sdk/client-sdk/go/crypto/signature"
	"github.com/oasisprotocol/oasis-sdk/client-sdk/go/types"
)

var (
	txOffline  bool
	txNonce    uint64
	txGasLimit uint64
)

// TransactionFlags contains the common transaction flags.
var TransactionFlags *flag.FlagSet

// TransactionConfig contains the transaction-related configuration from flags.
type TransactionConfig struct {
	// Offline is a flag indicating that no online queries are allowed.
	Offline bool
}

// GetTransactionConfig returns the transaction-related configuration from flags.
func GetTransactionConfig() *TransactionConfig {
	return &TransactionConfig{
		Offline: txOffline,
	}
}

// SignConsensusTransaction signs a consensus transaction.
func SignConsensusTransaction(
	ctx context.Context,
	net *config.Network,
	wallet wallet.Wallet,
	conn client.Connection,
	tx *consensusTx.Transaction,
) (*consensusTx.SignedTransaction, error) {
	// Default to passed values and do online estimation when possible.
	tx.Nonce = txNonce
	if tx.Fee == nil {
		tx.Fee = &consensusTx.Fee{}
	}
	tx.Fee.Gas = consensusTx.Gas(txGasLimit)

	if !txOffline {
		// Query nonce.
		nonce, err := conn.Consensus().GetSignerNonce(ctx, &consensus.GetSignerNonceRequest{
			AccountAddress: wallet.Address().ConsensusAddress(),
			Height:         consensus.HeightLatest,
		})
		if err != nil {
			return nil, fmt.Errorf("failed to query nonce: %w", err)
		}
		tx.Nonce = nonce

		// Gas estimation.
		gas, err := conn.Consensus().EstimateGas(ctx, &consensus.EstimateGasRequest{
			Signer:      wallet.ConsensusSigner().Public(),
			Transaction: tx,
		})
		if err != nil {
			return nil, fmt.Errorf("failed to estimate gas: %w", err)
		}
		tx.Fee.Gas = gas
	}

	// TODO: Gas price.

	// Sign the transaction.
	signer := wallet.ConsensusSigner()
	if signer == nil {
		return nil, fmt.Errorf("wallet does not support signing consensus transactions")
	}

	// NOTE: We build our own domain separation context here as we need to support multiple chain
	//       contexts at the same time. Would be great if chainContextSeparator was exposed in core.
	sigCtx := coreSignature.Context([]byte(fmt.Sprintf("%s for chain %s", consensusTx.SignatureContext, net.ChainContext)))
	signed, err := coreSignature.SignSigned(signer, sigCtx, tx)
	if err != nil {
		return nil, err
	}

	return &consensusTx.SignedTransaction{Signed: *signed}, nil
}

// SignParaTimeTransaction signs a ParaTime transaction.
func SignParaTimeTransaction(
	ctx context.Context,
	net *config.Network,
	pt *config.ParaTime,
	wallet wallet.Wallet,
	conn client.Connection,
	tx *types.Transaction,
) (*types.UnverifiedTransaction, error) {
	// Default to passed values and do online estimation when possible.
	nonce := txNonce
	tx.AuthInfo.Fee.Gas = txGasLimit

	if !txOffline {
		// Query nonce.
		var err error
		nonce, err = conn.Runtime(pt).Accounts.Nonce(ctx, client.RoundLatest, wallet.Address())
		if err != nil {
			return nil, fmt.Errorf("failed to query nonce: %w", err)
		}
	}

	// Prepare the transaction before (optional) gas estimation to ensure correct estimation.
	tx.AppendAuthSignature(wallet.SignatureAddressSpec(), nonce)

	if !txOffline {
		// Gas estimation.
		var err error
		tx.AuthInfo.Fee.Gas, err = conn.Runtime(pt).Core.EstimateGas(ctx, client.RoundLatest, tx)
		if err != nil {
			return nil, fmt.Errorf("failed to estimate gas: %w", err)
		}
	}

	// TODO: Gas price.

	// Sign the transaction.
	sigCtx := signature.DeriveChainContext(pt.Namespace(), net.ChainContext)
	ts := tx.PrepareForSigning()
	if err := ts.AppendSign(sigCtx, wallet.Signer()); err != nil {
		return nil, fmt.Errorf("failed to sign transaction: %w", err)
	}

	return ts.UnverifiedTransaction(), nil
}

// PrintSignedTransaction prints a signed transaction.
func PrintSignedTransaction(sigTx interface{}) {
	// TODO: Add some options for controlling output.
	formatted, err := json.MarshalIndent(sigTx, "", "  ")
	cobra.CheckErr(err)
	fmt.Println(string(formatted))
}

// BroadcastTransaction broadcasts a transaction.
//
// When in offline mode, it outputs the transaction instead.
func BroadcastTransaction(
	ctx context.Context,
	pt *config.ParaTime,
	conn client.Connection,
	tx interface{},
) error {
	if txOffline {
		PrintSignedTransaction(tx)
		return nil
	}

	switch sigTx := tx.(type) {
	case *consensusTx.SignedTransaction:
		// Consensus transaction.
		fmt.Printf("Broadcasting transaction...\n")
		err := conn.Consensus().SubmitTx(ctx, sigTx)
		cobra.CheckErr(err)

		fmt.Printf("Transaction executed successfully.\n")
		fmt.Printf("Transaction hash: %s\n", sigTx.Hash())

		return nil
	case *types.UnverifiedTransaction:
		// ParaTime transaction.
		fmt.Printf("Broadcasting transaction...\n")
		meta, err := conn.Runtime(pt).SubmitTxMeta(ctx, sigTx)
		cobra.CheckErr(err)

		fmt.Printf("Transaction executed successfully.\n")
		fmt.Printf("Round:            %d\n", meta.Round)
		fmt.Printf("Transaction hash: %s\n", sigTx.Hash())

		return nil
	default:
		return fmt.Errorf("unsupported transaction kind: %T", tx)
	}
}

func init() {
	TransactionFlags = flag.NewFlagSet("", flag.ContinueOnError)
	TransactionFlags.BoolVar(&txOffline, "offline", false, "do not perform any operations requiring network access")
	TransactionFlags.Uint64Var(&txNonce, "nonce", 0, "override nonce to use")
	TransactionFlags.Uint64Var(&txGasLimit, "gas-limit", 0, "override gas limit to use (disable estimation)")
}
