// Modern C# (C# 10+) sample using file-scoped namespace
using System;
using System.Collections.Generic;
using System.Threading.Tasks;

namespace BFG.Business.Services;

public interface IOrderRepository
{
    Task<OrderSummary?> GetByIdAsync(string orderId);
    Task SaveAsync(OrderSummary order);
}

public class OrderService
{
    private readonly IOrderRepository _repository;

    public string ServiceName { get; init; } = "OrderService";
    public bool IsInitialized { get; private set; }

    public OrderService(IOrderRepository repository)
    {
        _repository = repository;
        IsInitialized = true;
    }

    public async Task<OrderSummary?> GetOrderAsync(string orderId)
    {
        return await _repository.GetByIdAsync(orderId);
    }

    public decimal CalculateTotal(List<decimal> lineItems)
    {
        decimal total = 0;
        foreach (var item in lineItems)
            total += item;
        return total;
    }

    public bool IsValid(string orderId)
    {
        return !string.IsNullOrWhiteSpace(orderId);
    }
}

public record OrderSummary(string OrderId, decimal Total, DateTime CreatedAt);

public record OrderDetails
{
    public string OrderId { get; init; } = string.Empty;
    public decimal Total { get; init; }
    public string CustomerName { get; init; } = string.Empty;
    public DateTime CreatedAt { get; init; }
}

public enum OrderStatus
{
    Pending,
    Processing,
    Shipped,
    Delivered,
    Cancelled,
}
