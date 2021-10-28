package cmd

import (
	"context"
	"fmt"

	"github.com/spf13/cobra"

	consensus "github.com/oasisprotocol/oasis-core/go/consensus/api"
	staking "github.com/oasisprotocol/oasis-core/go/staking/api"

	"github.com/oasisprotocol/oasis-sdk/cli/client"
	"github.com/oasisprotocol/oasis-sdk/cli/cmd/common"
	"github.com/oasisprotocol/oasis-sdk/cli/config"
	sdkClient "github.com/oasisprotocol/oasis-sdk/client-sdk/go/client"
	"github.com/oasisprotocol/oasis-sdk/client-sdk/go/modules/consensusaccounts"
	"github.com/oasisprotocol/oasis-sdk/client-sdk/go/types"
)

var (
	accountsCmd = &cobra.Command{
		Use:   "accounts",
		Short: "Account operations",
	}

	accountsShowCmd = &cobra.Command{
		Use:   "show [address]",
		Short: "Show account information",
		Args:  cobra.MaximumNArgs(1),
		Run: func(cmd *cobra.Command, args []string) {
			cfg := config.Global()
			npw := common.GetNPWSelection(cfg)

			// Determine which address to show. If an explicit argument was given, use that
			// otherwise use the selected wallet.
			var targetAddress string
			switch {
			case len(args) >= 1:
				// Explicit argument given.
				targetAddress = args[0]
			case npw.Wallet != nil:
				// Wallet is selected.
				targetAddress = npw.Wallet.Address
			default:
				// No address given and no wallets configured.
				cobra.CheckErr("no address given and no wallets configured")
			}

			// Establish connection with the target network.
			ctx := context.Background()
			c, err := client.Connect(ctx, npw.Network)
			cobra.CheckErr(err)

			addr, err := config.ResolveAddress(npw.Network, targetAddress)
			cobra.CheckErr(err)

			// Query consensus layer account.
			// TODO: Nicer overall formatting.
			fmt.Printf("Address: %s\n", addr)
			fmt.Println()
			fmt.Printf("=== CONSENSUS LAYER (%s) ===\n", npw.NetworkName)

			consensusAccount, err := c.Consensus().Staking().Account(ctx, &staking.OwnerQuery{
				Height: consensus.HeightLatest,
				Owner:  addr.ConsensusAddress(),
			})
			cobra.CheckErr(err)

			// TODO: Pretty printing of units based on introspection queries.
			fmt.Printf("Balance: %s\n", common.FormatConsensusDenomination(npw.Network, consensusAccount.General.Balance))

			if npw.ParaTime != nil {
				// Query runtime account when a paratime has been configured.
				fmt.Println()
				fmt.Printf("=== %s PARATIME ===\n", npw.ParaTimeName)

				rtBalances, err := c.Runtime(npw.ParaTime).Accounts.Balances(ctx, client.RoundLatest, *addr)
				cobra.CheckErr(err)

				fmt.Printf("Balances for all denominations:\n")
				for denom, balance := range rtBalances.Balances {
					fmt.Printf("  %s\n", common.FormatParaTimeDenomination(npw.ParaTime, types.NewBaseUnits(balance, denom)))
				}
			}
		},
	}

	accountsAllowCmd = &cobra.Command{
		Use:   "allow [beneficiary] [amount]",
		Short: "Configure beneficiary allowance for an account",
		Args:  cobra.ExactArgs(2),
		Run: func(cmd *cobra.Command, args []string) {
			cfg := config.Global()
			npw := common.GetNPWSelection(cfg)
			txCfg := common.GetTransactionConfig()
			beneficiary, amount := args[0], args[1]

			if npw.Wallet == nil {
				cobra.CheckErr("no wallets configured")
			}

			// When not in offline mode, connect to the given network endpoint.
			ctx := context.Background()
			var conn client.Connection
			if !txCfg.Offline {
				var err error
				conn, err = client.Connect(ctx, npw.Network)
				cobra.CheckErr(err)
			}

			// Resolve beneficiary address.
			benAddr, err := config.ResolveAddress(npw.Network, beneficiary)
			cobra.CheckErr(err)

			// Parse amount.
			var negative bool
			if amount[0] == '-' {
				negative = true
				amount = amount[1:]
			}
			amountChange, err := common.ParseConsensusDenomination(npw.Network, amount)
			cobra.CheckErr(err)

			// Prepare transaction.
			tx := staking.NewAllowTx(0, nil, &staking.Allow{
				Beneficiary:  benAddr.ConsensusAddress(),
				Negative:     negative,
				AmountChange: *amountChange,
			})

			wallet := loadWallet(npw.WalletName)
			sigTx, err := common.SignConsensusTransaction(ctx, npw.Network, wallet, conn, tx)
			cobra.CheckErr(err)

			common.BroadcastTransaction(ctx, npw.ParaTime, conn, sigTx)
		},
	}

	accountsDepositCmd = &cobra.Command{
		Use:   "deposit [amount] [to]",
		Short: "Deposit given amount of tokens into an account in the ParaTime",
		Args:  cobra.RangeArgs(1, 2),
		Run: func(cmd *cobra.Command, args []string) {
			cfg := config.Global()
			npw := common.GetNPWSelection(cfg)
			txCfg := common.GetTransactionConfig()
			amount := args[0]
			var to string
			if len(args) >= 2 {
				to = args[1]
			}

			if npw.Wallet == nil {
				cobra.CheckErr("no wallets configured")
			}
			if npw.ParaTime == nil {
				cobra.CheckErr("no paratimes to deposit into")
			}

			// When not in offline mode, connect to the given network endpoint.
			ctx := context.Background()
			var conn client.Connection
			if !txCfg.Offline {
				var err error
				conn, err = client.Connect(ctx, npw.Network)
				cobra.CheckErr(err)
			}

			// Resolve destination address when specified.
			var toAddr *types.Address
			if to != "" {
				var err error
				toAddr, err = config.ResolveAddress(npw.Network, to)
				cobra.CheckErr(err)
			}

			// Parse amount.
			// TODO: This should actually query the ParaTime (or config) to check what the consensus
			//       layer denomination is in the ParaTime. Assume NATIVE for now.
			amountBaseUnits, err := common.ParseParaTimeDenomination(npw.ParaTime, amount, types.NativeDenomination)
			cobra.CheckErr(err)

			// Prepare transaction.
			tx := consensusaccounts.NewDepositTx(nil, &consensusaccounts.Deposit{
				To:     toAddr,
				Amount: *amountBaseUnits,
			})

			wallet := loadWallet(npw.WalletName)
			sigTx, err := common.SignParaTimeTransaction(ctx, npw.Network, npw.ParaTime, wallet, conn, tx)
			cobra.CheckErr(err)

			if txCfg.Offline {
				common.PrintSignedTransaction(sigTx)
				return
			}

			var ch <-chan *sdkClient.BlockEvents
			ch, err = conn.Runtime(npw.ParaTime).WatchEvents(ctx, []sdkClient.EventDecoder{
				conn.Runtime(npw.ParaTime).ConsensusAccounts,
			}, false)
			cobra.CheckErr(err)

			common.BroadcastTransaction(ctx, npw.ParaTime, conn, sigTx)

			fmt.Printf("Waiting for deposit result...\n")

			for {
				select {
				case bev := <-ch:
					for _, ev := range bev.Events {
						ce, ok := ev.(*consensusaccounts.Event)
						if !ok || ce.Deposit == nil {
							continue
						}
						if !ce.Deposit.From.Equal(wallet.Address()) || ce.Deposit.Nonce != tx.AuthInfo.SignerInfo[0].Nonce {
							continue
						}

						// Check for result.
						switch ce.Deposit.IsSuccess() {
						case true:
							fmt.Printf("Deposit succeeded.\n")
						case false:
							cobra.CheckErr(fmt.Errorf("Deposit failed with error code %d from module %s.",
								ce.Deposit.Error.Code,
								ce.Deposit.Error.Module,
							))
						}
						return
					}

					// TODO: Timeout.
				}
			}
		},
	}

	accountsWithdrawCmd = &cobra.Command{
		Use:   "withdraw [amount] [to]",
		Short: "Withdraw given amount of tokens into an account in the consensus layer",
		Args:  cobra.RangeArgs(1, 2),
		Run: func(cmd *cobra.Command, args []string) {
			cfg := config.Global()
			npw := common.GetNPWSelection(cfg)
			txCfg := common.GetTransactionConfig()
			amount := args[0]
			var to string
			if len(args) >= 2 {
				to = args[1]
			}

			if npw.Wallet == nil {
				cobra.CheckErr("no wallets configured")
			}
			if npw.ParaTime == nil {
				cobra.CheckErr("no paratimes to withdraw from")
			}

			// When not in offline mode, connect to the given network endpoint.
			ctx := context.Background()
			var conn client.Connection
			if !txCfg.Offline {
				var err error
				conn, err = client.Connect(ctx, npw.Network)
				cobra.CheckErr(err)
			}

			// Resolve destination address when specified.
			var toAddr *types.Address
			if to != "" {
				var err error
				toAddr, err = config.ResolveAddress(npw.Network, to)
				cobra.CheckErr(err)
			}

			// Parse amount.
			// TODO: This should actually query the ParaTime (or config) to check what the consensus
			//       layer denomination is in the ParaTime. Assume NATIVE for now.
			amountBaseUnits, err := common.ParseParaTimeDenomination(npw.ParaTime, amount, types.NativeDenomination)
			cobra.CheckErr(err)

			// Prepare transaction.
			tx := consensusaccounts.NewWithdrawTx(nil, &consensusaccounts.Withdraw{
				To:     toAddr,
				Amount: *amountBaseUnits,
			})

			wallet := loadWallet(npw.WalletName)
			sigTx, err := common.SignParaTimeTransaction(ctx, npw.Network, npw.ParaTime, wallet, conn, tx)
			cobra.CheckErr(err)

			common.BroadcastTransaction(ctx, npw.ParaTime, conn, sigTx)
		},
	}
)

func init() {
	accountsShowCmd.Flags().AddFlagSet(common.SelectorFlags)

	accountsAllowCmd.Flags().AddFlagSet(common.SelectorFlags)
	accountsAllowCmd.Flags().AddFlagSet(common.TransactionFlags)

	accountsDepositCmd.Flags().AddFlagSet(common.SelectorFlags)
	accountsDepositCmd.Flags().AddFlagSet(common.TransactionFlags)

	accountsWithdrawCmd.Flags().AddFlagSet(common.SelectorFlags)
	accountsWithdrawCmd.Flags().AddFlagSet(common.TransactionFlags)

	accountsCmd.AddCommand(accountsShowCmd)
	accountsCmd.AddCommand(accountsAllowCmd)
	accountsCmd.AddCommand(accountsDepositCmd)
	accountsCmd.AddCommand(accountsWithdrawCmd)
}
