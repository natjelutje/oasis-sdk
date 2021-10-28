module github.com/oasisprotocol/oasis-sdk/cli

go 1.16

replace github.com/oasisprotocol/oasis-sdk/client-sdk/go => ../client-sdk/go

require (
	github.com/adrg/xdg v0.4.0
	github.com/mitchellh/mapstructure v1.4.2
	github.com/oasisprotocol/deoxysii v0.0.0-20200527154044-851aec403956
	github.com/oasisprotocol/oasis-core/go v0.2103.6
	github.com/oasisprotocol/oasis-sdk/client-sdk/go v0.1.0
	github.com/olekukonko/tablewriter v0.0.5
	github.com/shopspring/decimal v1.3.1
	github.com/spf13/cobra v1.2.1
	github.com/spf13/pflag v1.0.5
	github.com/spf13/viper v1.9.0
	github.com/tyler-smith/go-bip39 v1.1.0
	golang.org/x/crypto v0.0.0-20210817164053-32db794688a5
	golang.org/x/term v0.0.0-20201126162022-7de9c90e9dd1
	google.golang.org/grpc v1.41.0
)
