package cmd

import (
	"fmt"
	"sort"

	"github.com/spf13/cobra"

	"github.com/oasisprotocol/oasis-sdk/cli/config"
	"github.com/oasisprotocol/oasis-sdk/cli/table"
)

var (
	networkCmd = &cobra.Command{
		Use:   "network",
		Short: "Manage network endpoints",
	}

	networkListCmd = &cobra.Command{
		Use:   "list",
		Short: "List configured networks",
		Run: func(cmd *cobra.Command, args []string) {
			cfg := config.Global()
			table := table.New()
			table.SetHeader([]string{"Name", "Chain Context", "RPC"})

			var output [][]string
			for name, net := range cfg.Networks.All {
				output = append(output, []string{
					name,
					net.ChainContext,
					net.RPC,
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

	networkAddCmd = &cobra.Command{
		Use:   "add [name] [chain-context] [rpc-endpoint]",
		Short: "Add a new network",
		Args:  cobra.ExactArgs(3),
		Run: func(cmd *cobra.Command, args []string) {
			cfg := config.Global()
			name, chainContext, rpc := args[0], args[1], args[2]

			err := cfg.Networks.Add(name, &config.Network{
				ChainContext: chainContext,
				RPC:          rpc,
				Denomination: config.DenominationInfo{
					Symbol:   "", // TODO: Support custom symbol specification.
					Decimals: 9,  // TODO: Support custom decimals specification.
				},
			})
			cobra.CheckErr(err)

			err = cfg.Save()
			cobra.CheckErr(err)
		},
	}

	networkRmCmd = &cobra.Command{
		Use:   "rm [name]",
		Short: "Remove an existing network",
		Args:  cobra.ExactArgs(1),
		Run: func(cmd *cobra.Command, args []string) {
			cfg := config.Global()
			name := args[0]

			err := cfg.Networks.Remove(name)
			cobra.CheckErr(err)

			err = cfg.Save()
			cobra.CheckErr(err)
		},
	}

	networkSetDefaultCmd = &cobra.Command{
		Use:   "set-default [name]",
		Short: "Sets the given network as the default network",
		Args:  cobra.ExactArgs(1),
		Run: func(cmd *cobra.Command, args []string) {
			cfg := config.Global()
			name := args[0]

			err := cfg.Networks.SetDefault(name)
			cobra.CheckErr(err)

			err = cfg.Save()
			cobra.CheckErr(err)
		},
	}

	networkSetRPCCmd = &cobra.Command{
		Use:   "set-rpc [name] [rpc-endpoint]",
		Short: "Sets the RPC endpoint of the given network",
		Args:  cobra.ExactArgs(2),
		Run: func(cmd *cobra.Command, args []string) {
			cfg := config.Global()
			name, rpc := args[0], args[1]

			net := cfg.Networks.All[name]
			if net == nil {
				cobra.CheckErr(fmt.Errorf("network '%s' does not exist", name))
			}

			net.RPC = rpc

			err := cfg.Save()
			cobra.CheckErr(err)
		},
	}
)

func init() {
	networkCmd.AddCommand(networkListCmd)
	networkCmd.AddCommand(networkAddCmd)
	networkCmd.AddCommand(networkRmCmd)
	networkCmd.AddCommand(networkSetDefaultCmd)
	networkCmd.AddCommand(networkSetRPCCmd)
}
