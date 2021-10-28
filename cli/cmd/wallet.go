package cmd

import (
	"bufio"
	"fmt"
	"os"
	"sort"

	"github.com/spf13/cobra"
	flag "github.com/spf13/pflag"
	"golang.org/x/term"

	"github.com/oasisprotocol/oasis-sdk/cli/config"
	"github.com/oasisprotocol/oasis-sdk/cli/table"
	"github.com/oasisprotocol/oasis-sdk/cli/wallet"
)

var (
	walletKind string

	walletCmd = &cobra.Command{
		Use:   "wallet",
		Short: "Manage wallets",
	}

	walletListCmd = &cobra.Command{
		Use:   "list",
		Short: "List configured wallets",
		Run: func(cmd *cobra.Command, args []string) {
			cfg := config.Global()
			table := table.New()
			table.SetHeader([]string{"Name", "Kind", "Address"})

			var output [][]string
			for name, wallet := range cfg.Wallets.All {
				output = append(output, []string{
					name,
					wallet.Kind,
					wallet.Address,
				})
			}

			// Sort output by name.
			sort.Slice(output, func(i, j int) bool {
				return output[i][0] < output[j][0]
			})

			table.AppendBulk(output)
			table.Render()
		},
	}

	walletCreateCmd = &cobra.Command{
		Use:   "create [name]",
		Short: "Create a new wallet",
		Args:  cobra.ExactArgs(1),
		Run: func(cmd *cobra.Command, args []string) {
			cfg := config.Global()
			name := args[0]

			// Ask for passphrase to encrypt the wallet with.
			fmt.Printf("Passphrase: ")
			passphrase, err := term.ReadPassword(int(os.Stdin.Fd()))
			fmt.Printf("\n")
			cobra.CheckErr(err)

			walletCfg := &config.Wallet{
				Kind: walletKind,
			}
			err = walletCfg.SetConfigFromFlags()
			cobra.CheckErr(err)

			err = cfg.Wallets.Create(name, string(passphrase), walletCfg)
			cobra.CheckErr(err)

			err = cfg.Save()
			cobra.CheckErr(err)
		},
	}

	walletShowCmd = &cobra.Command{
		Use:   "show [name]",
		Short: "Show public wallet information",
		Args:  cobra.ExactArgs(1),
		Run: func(cmd *cobra.Command, args []string) {
			name := args[0]

			wallet := loadWallet(name)
			showPublicWalletInfo(wallet)
		},
	}

	walletExportCmd = &cobra.Command{
		Use:   "export [name]",
		Short: "Export secret wallet information",
		Args:  cobra.ExactArgs(1),
		Run: func(cmd *cobra.Command, args []string) {
			name := args[0]

			fmt.Printf("WARNING: Exporting the wallet will expose secret key material!\n")
			wallet := loadWallet(name)

			showPublicWalletInfo(wallet)

			fmt.Printf("Export:\n")
			fmt.Println(wallet.UnsafeExport())
		},
	}

	walletRmCmd = &cobra.Command{
		Use:   "rm [name]",
		Short: "Remove an existing wallet",
		Args:  cobra.ExactArgs(1),
		Run: func(cmd *cobra.Command, args []string) {
			cfg := config.Global()
			name := args[0]

			// Early check for whether the wallet exists so that we don't ask for confirmation first.
			if _, exists := cfg.Wallets.All[name]; !exists {
				cobra.CheckErr(fmt.Errorf("wallet '%s' does not exist", name))
			}

			confirmText := fmt.Sprintf("I really want to remove wallet %s", name)

			fmt.Printf("WARNING: Removing the wallet will ERASE secret key material!\n")
			fmt.Printf("WARNING: THIS ACTION IS IRREVERSIBLE!\n")
			fmt.Printf("Enter '%s' (without quotes) to confirm removal:\n", confirmText)

			scanner := bufio.NewScanner(os.Stdin)
			scanner.Scan()
			if scanner.Text() != confirmText {
				cobra.CheckErr("Aborted.")
			}

			err := cfg.Wallets.Remove(name)
			cobra.CheckErr(err)

			err = cfg.Save()
			cobra.CheckErr(err)
		},
	}

	walletSetDefaultCmd = &cobra.Command{
		Use:   "set-default [name]",
		Short: "Sets the given wallet as the default wallet",
		Args:  cobra.ExactArgs(1),
		Run: func(cmd *cobra.Command, args []string) {
			cfg := config.Global()
			name := args[0]

			err := cfg.Wallets.SetDefault(name)
			cobra.CheckErr(err)

			err = cfg.Save()
			cobra.CheckErr(err)
		},
	}
)

func loadWallet(name string) wallet.Wallet {
	cfg := config.Global()

	// Early check for whether the wallet exists so that we don't ask for passphrase first.
	if _, exists := cfg.Wallets.All[name]; !exists {
		cobra.CheckErr(fmt.Errorf("wallet '%s' does not exist", name))
	}

	// Ask for passphrase to decrypt the wallet.
	fmt.Printf("Passphrase: ")
	passphrase, err := term.ReadPassword(int(os.Stdin.Fd()))
	fmt.Printf("\n")
	cobra.CheckErr(err)

	wallet, err := cfg.Wallets.Load(name, string(passphrase))
	cobra.CheckErr(err)

	return wallet
}

func showPublicWalletInfo(wallet wallet.Wallet) {
	fmt.Printf("Public Key: %s\n", wallet.Signer().Public())
	fmt.Printf("Address:    %s\n", wallet.Address())
}

func init() {
	walletCmd.AddCommand(walletListCmd)

	walletFlags := flag.NewFlagSet("", flag.ContinueOnError)
	// TODO: Dynamically populate supported wallet kinds.
	walletFlags.StringVar(&walletKind, "kind", "file", "wallet kind")

	// TODO: Group flags in usage by tweaking the usage template/function.
	for _, wf := range wallet.AvailableKinds() {
		walletFlags.AddFlagSet(wf.Flags())
	}

	walletCreateCmd.Flags().AddFlagSet(walletFlags)

	walletCmd.AddCommand(walletCreateCmd)
	walletCmd.AddCommand(walletShowCmd)
	walletCmd.AddCommand(walletExportCmd)
	walletCmd.AddCommand(walletRmCmd)
	walletCmd.AddCommand(walletSetDefaultCmd)
}
