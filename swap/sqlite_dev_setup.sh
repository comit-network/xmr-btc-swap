# crated temporary DB
# run the migration scripts to create the tables
# prepare the sqlx-data.json rust mappings
DATABASE_URL=sqlite:tempdb cargo sqlx database create
DATABASE_URL=sqlite:tempdb cargo sqlx migrate run
DATABASE_URL=sqlite:./swap/tempdb cargo sqlx prepare -- --bin swap