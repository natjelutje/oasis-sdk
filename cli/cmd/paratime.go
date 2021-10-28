package cmd

import (
	"fmt"
	"sort"

	"github.com/spf13/cobra"

	"github.com/oasisprotocol/oasis-sdk/cli/config"
	"github.com/oasisprotocol/oasis-sdk/cli/table"
)

var (
	paratimeCmd = &cobra.Command{
		Use:   "paratime",
		Short: "Manage paratimes",
	}

	paratimeListCmd = &cobra.Command{
		Use:   "list",
		Short: "List configured paratimes",
		Run: func(cmd *cobra.Command, args []string) {
			cfg := config.Global()
			table := table.New()
			table.SetHeader([]string{"Network", "Paratime", "ID"})

			var output [][]string
			for netName, net := range cfg.Networks.All {
				for ptName, pt := range net.ParaTimes.All {
					output = append(output, []string{
						netName,
						ptName,
						pt.ID,
					})
				}
			}

			// Sort output by network name and paratime name.
			sort.Slice(output, func(i, j int) bool {
				if output[i][0] != output[j][0] {
					return output[i][0] < output[j][0]
				}
				return output[i][1] < output[j][1]
			})

			table.AppendBulk(output)
			table.Render()
		},
	}

	paratimeAddCmd = &cobra.Command{
		Use:   "add [network] [name] [id]",
		Short: "Add a new paratime",
		Args:  cobra.ExactArgs(3),
		Run: func(cmd *cobra.Command, args []string) {
			cfg := config.Global()
			network, name, id := args[0], args[1], args[2]

			net, exists := cfg.Networks.All[network]
			if !exists {
				cobra.CheckErr(fmt.Errorf("network '%s' does not exist", network))
			}

			err := net.ParaTimes.Add(name, &config.ParaTime{
				ID: id,
				Denominations: map[string]*config.DenominationInfo{
					config.NativeDenominationKey: {
						Symbol:   net.Denomination.Symbol,   // TODO: Support custom symbol specification.
						Decimals: net.Denomination.Decimals, // TODO: Support custom decimals specification.
					},
				},
			})
			cobra.CheckErr(err)

			err = cfg.Save()
			cobra.CheckErr(err)
		},
	}

	paratimeRmCmd = &cobra.Command{
		Use:   "rm [network] [name]",
		Short: "Remove an existing paratime",
		Args:  cobra.ExactArgs(2),
		Run: func(cmd *cobra.Command, args []string) {
			cfg := config.Global()
			network, name := args[0], args[1]

			net, exists := cfg.Networks.All[network]
			if !exists {
				cobra.CheckErr(fmt.Errorf("network '%s' does not exist", network))
			}

			err := net.ParaTimes.Remove(name)
			cobra.CheckErr(err)

			err = cfg.Save()
			cobra.CheckErr(err)
		},
	}

	paratimeSetDefaultCmd = &cobra.Command{
		Use:   "set-default [network] [name]",
		Short: "Sets the given paratime as the default paratime for the given network",
		Args:  cobra.ExactArgs(2),
		Run: func(cmd *cobra.Command, args []string) {
			cfg := config.Global()
			network, name := args[0], args[1]

			net, exists := cfg.Networks.All[network]
			if !exists {
				cobra.CheckErr(fmt.Errorf("network '%s' does not exist", network))
			}

			err := net.ParaTimes.SetDefault(name)
			cobra.CheckErr(err)

			err = cfg.Save()
			cobra.CheckErr(err)
		},
	}
)

func init() {
	paratimeCmd.AddCommand(paratimeListCmd)
	paratimeCmd.AddCommand(paratimeAddCmd)
	paratimeCmd.AddCommand(paratimeRmCmd)
	paratimeCmd.AddCommand(paratimeSetDefaultCmd)
}
