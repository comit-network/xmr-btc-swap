import { Autocomplete, Box, TextField, Typography } from "@mui/material";

export default function ReceiveAddressSelector() {
    return (
        <Box sx={{
            display: "flex",
            flexDirection: "row",
            alignItems: "center",
            gap: 2,
            width: "100%",
        }}>
            <Typography variant="body1">Receive Address</Typography>
            <Autocomplete
                sx={{
                    flexGrow: 1,
                }}
                options={[]}
                renderInput={(params) => <TextField {...params} label="Receive Address" fullWidth />}
            />
        </Box>
    )
}