package accounts

import (
	"context"
	"fmt"

	"github.com/oasisprotocol/oasis-core/go/common/cbor"

	"github.com/oasisprotocol/oasis-sdk/client-sdk/go/client"
	"github.com/oasisprotocol/oasis-sdk/client-sdk/go/types"
)

const (
	// Callable methods.
	methodTransfer = "accounts.Transfer"

	// Queries.
	methodNonce            = "accounts.Nonce"
	methodBalances         = "accounts.Balances"
	methodAddresses        = "accounts.Addresses"
	methodDenominationInfo = "accounts.DenominationInfo"
)

// V1 is the v1 accounts module interface.
type V1 interface {
	client.EventDecoder

	// Transfer generates an accounts.Transfer transaction.
	Transfer(to types.Address, amount types.BaseUnits) *client.TransactionBuilder

	// Nonce queries the given account's nonce.
	Nonce(ctx context.Context, round uint64, address types.Address) (uint64, error)

	// Balances queries the given account's balances.
	Balances(ctx context.Context, round uint64, address types.Address) (*AccountBalances, error)

	// Addresses queries all account addresses.
	Addresses(ctx context.Context, round uint64, denomination types.Denomination) (Addresses, error)

	// DenominationInfo queries the information about a given denomination.
	DenominationInfo(ctx context.Context, round uint64, denomination types.Denomination) (*DenominationInfo, error)

	// GetEvents returns all account events emitted in a given block.
	GetEvents(ctx context.Context, round uint64) ([]*Event, error)
}

type v1 struct {
	rc client.RuntimeClient
}

// Implements V1.
func (a *v1) Transfer(to types.Address, amount types.BaseUnits) *client.TransactionBuilder {
	return client.NewTransactionBuilder(a.rc, methodTransfer, &Transfer{
		To:     to,
		Amount: amount,
	})
}

// Implements V1.
func (a *v1) Nonce(ctx context.Context, round uint64, address types.Address) (uint64, error) {
	var nonce uint64
	err := a.rc.Query(ctx, round, methodNonce, &NonceQuery{Address: address}, &nonce)
	if err != nil {
		return 0, err
	}
	return nonce, nil
}

// Implements V1.
func (a *v1) Balances(ctx context.Context, round uint64, address types.Address) (*AccountBalances, error) {
	var balances AccountBalances
	err := a.rc.Query(ctx, round, methodBalances, &BalancesQuery{Address: address}, &balances)
	if err != nil {
		return nil, err
	}
	return &balances, nil
}

// Implements V1.
func (a *v1) Addresses(ctx context.Context, round uint64, denomination types.Denomination) (Addresses, error) {
	var addresses Addresses
	err := a.rc.Query(ctx, round, methodAddresses, &AddressesQuery{Denomination: denomination}, &addresses)
	if err != nil {
		return nil, err
	}
	return addresses, nil
}

// Implements V1.
func (a *v1) DenominationInfo(ctx context.Context, round uint64, denomination types.Denomination) (*DenominationInfo, error) {
	var info DenominationInfo
	err := a.rc.Query(ctx, round, methodDenominationInfo, &DenominationInfoQuery{Denomination: denomination}, &info)
	if err != nil {
		return nil, err
	}
	return &info, nil
}

// Implements V1.
func (a *v1) GetEvents(ctx context.Context, round uint64) ([]*Event, error) {
	rawEvs, err := a.rc.GetEventsRaw(ctx, round)
	if err != nil {
		return nil, err
	}

	evs := make([]*Event, 0)
	for _, rawEv := range rawEvs {
		ev, err := a.DecodeEvent(rawEv)
		if err != nil {
			return nil, err
		}
		if ev == nil {
			continue
		}
		evs = append(evs, ev.(*Event))
	}

	return evs, nil
}

// Implements client.EventDecoder.
func (a *v1) DecodeEvent(event *types.Event) (client.DecodedEvent, error) {
	if event.Module != ModuleName {
		return nil, nil
	}
	switch event.Code {
	case TransferEventCode:
		var ev *TransferEvent
		if err := cbor.Unmarshal(event.Value, &ev); err != nil {
			return nil, fmt.Errorf("decode account transfer event value: %w", err)
		}
		return &Event{
			Transfer: ev,
		}, nil
	case BurnEventCode:
		var ev *BurnEvent
		if err := cbor.Unmarshal(event.Value, &ev); err != nil {
			return nil, fmt.Errorf("decode account burn event value: %w", err)
		}
		return &Event{
			Burn: ev,
		}, nil
	case MintEventCode:
		var ev *MintEvent
		if err := cbor.Unmarshal(event.Value, &ev); err != nil {
			return nil, fmt.Errorf("decode account mint event value: %w", err)
		}
		return &Event{
			Mint: ev,
		}, nil
	default:
		return nil, fmt.Errorf("invalid accounts event code: %v", event.Code)
	}
}

// NewV1 generates a V1 client helper for the accounts module.
func NewV1(rc client.RuntimeClient) V1 {
	return &v1{rc: rc}
}

// NewTransferTx generates a new accounts.Transfer transaction.
func NewTransferTx(fee *types.Fee, body *Transfer) *types.Transaction {
	return types.NewTransaction(fee, methodTransfer, body)
}
